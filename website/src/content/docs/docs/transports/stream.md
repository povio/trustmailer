---
title: Stream transport
description: Rendering a message to raw MIME bytes without delivering it anywhere — for tests, custom relays, and hand-rolled delivery.
---

The stream transport renders a message to the exact same raw RFC 5322 byte buffer every
real transport builds (via the shared `mime::build()`), but never sends it anywhere. It
mirrors nodemailer's `streamTransport`, and exists for three overlapping use cases:
asserting on the exact bytes a message would produce in tests, handing the rendered
bytes off to your own delivery mechanism (a custom relay, a queue, an HTTP call you
make yourself), or getting mail working end-to-end on a runtime that has no other
delivery transport available. It's implemented in `trustmailer-stream`, re-exported from
the facade crate as `trustmailer::stream` behind the `stream` Cargo feature (on by
default). It does zero I/O, so it's available on every runtime, including
`trustmailer-wasm`.

## Configuration

| Field      | Type     | Default       | Description |
| ---------- | -------- | -------------- | ----------- |
| `hostname` | `String` | `"localhost"`  | Hostname used only for generating a `Message-ID` when a message doesn't already have one set. |

Unlike the sendmail transport (which hard-codes `"localhost"` with no way to change it),
`StreamTransport` lets you configure this hostname — but note the constructor shape:
`StreamTransport::with_hostname(hostname)` is an alternate constructor, **not** a
chainable builder method on an existing instance. Use either
`StreamTransport::new()` (hostname `"localhost"`) or
`StreamTransport::with_hostname("mail.example.com")` — not both together.

## `render()` vs `send()`

`StreamTransport` exposes a plain, synchronous `render(&self, message: &Message) ->
Result<Vec<u8>>` method in addition to the `Transport::send()` trait method. `render()`
is the more direct tool for tests and inspection — it just returns the bytes, with no
envelope computation or `SentInfo` wrapping. `send()` calls `render()` internally and
additionally computes the effective envelope (resolved `From`/`To`/`Cc`/`Bcc`), so use
`send()` when you specifically want a `Transport`-shaped result (for example, to run the
exact same code path you'd use with SMTP/sendmail/SES in a test, swapping only the
transport).

## `SentInfo` shape

- `raw` — always `Some(bytes)`: the full rendered RFC 5322 message, headers and body,
  exactly as any real transport would have sent it. This is the field to inspect in
  tests — assert on `info.raw` to check headers, encoding, attachment boundaries, etc.
- `response` — always the fixed string `"250 stream transport: message rendered, not
  sent"`. It is not the rendered message; don't parse it looking for MIME content — read
  `raw` instead.
- `accepted` / `envelope` — computed the same way as every other transport, from
  `message.effective_envelope()`, so recipient-resolution logic can be tested the same
  way whether or not mail is actually delivered.
- `rejected` — always empty; there is nothing to reject since nothing was sent.
- `message_id` — the `Message`'s own `message_id` field if you set one, `""` otherwise.
  As with the sendmail transport, the *rendered* MIME in `raw` always has a generated
  `Message-ID` header (using `hostname`) even when the `Message` itself had none — but
  `SentInfo.message_id` only reflects what was on the `Message` struct going in, not
  what ended up in the rendered bytes.
- A `Message` with `msg.raw` already set (see `Message::builder()` / manually assigning
  `.raw`) passes that override straight through byte-for-byte, unmodified, bypassing
  `mime::build()` entirely.

## Rust example

```rust
use trustmailer::{Message, Transport};
use trustmailer::stream::StreamTransport;

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let transport = StreamTransport::with_hostname("example.com");

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .html("<b>Hi Bob!</b>")
        .build();

    let info = transport.send(&message).await?;
    let rendered = info.raw.expect("stream transport always sets raw");
    println!("{}", String::from_utf8_lossy(&rendered));
    Ok(())
}
```

A test that only needs the bytes, with no envelope handling, can call `render()`
directly instead:

```rust
let transport = StreamTransport::new();
let bytes = transport.render(&message)?;
assert!(String::from_utf8_lossy(&bytes).contains("Hi Bob!"));
```

## Node.js, Bun, Deno

`trustmailer-node`'s `createTransport()` exposes this transport with `streamTransport:
true`. There is currently no way to set a custom `hostname` from the Node binding — that
constructor exists only on the Rust `StreamTransport` itself.

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({ streamTransport: true });

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Runtime check",
  text: "hello",
});
console.log(info.response); // "250 stream transport: message rendered, not sent"
```

:::caution[The Node/WASM bindings don't currently return the rendered bytes]
Reading the binding source closely matters here: `StreamTransport::send()` sets
`SentInfo.response` to that fixed confirmation string and puts the actual rendered
bytes in `SentInfo.raw` instead — and neither `trustmailer-node`'s `SentInfoJs` nor
`trustmailer-wasm`'s JS-facing result type currently exposes a `raw` field at all. So
today, calling the stream transport from Node.js, Bun, Deno, or a WASM host gets you
delivery metadata (`accepted`, `rejected`, `messageId`) and that fixed string — not the
rendered MIME. If you need the actual bytes back in JS, call `render()` from Rust in a
native module, or use the [JSON transport](/docs/transports/json/) with the same
caveat in mind, or drive `trustmailer` from native Rust directly.
:::

## See also

- [JSON transport](/docs/transports/json/) — a structured alternative rendering, also
  for tests.
- [Sendmail transport](/docs/transports/sendmail/) and [SES transport](/docs/transports/ses/)
  — the real delivery transports this one is a stand-in for.
- [Security](/docs/security/) — the CRLF-sanitizing header path this transport
  exercises the same as any real transport, since it shares `mime::build()`.
- [Runtime support matrix](/docs/runtime-matrix/).
