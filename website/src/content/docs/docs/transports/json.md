---
title: JSON transport
description: Serializing a message to a structured JSON object for tests and hand-off to your own delivery code — not a stand-in for MIME safety testing.
---

The JSON transport serializes a message into a small JSON object instead of building
RFC 5322 MIME or delivering it anywhere. It mirrors nodemailer's `jsonTransport`, and is
aimed squarely at tests and at runtimes where you'd rather hand the message off to
something else (your own HTTP relay, a browser-side preview, a queue) than have
trustmailer format it as MIME at all. It's implemented in `trustmailer-json`,
re-exported from the facade crate as `trustmailer::json` behind the `json` Cargo feature
(on by default). Like the stream transport, it does zero I/O and is available on every
runtime, including `trustmailer-wasm`.

## Configuration

`JsonTransport` takes no configuration — `JsonTransport::new()` (or `JsonTransport`'s
`Default` impl) is the whole surface. There's no equivalent of the sendmail transport's
`path`/`args` or the stream transport's `hostname` to set.

## What gets serialized

`JsonTransport::render()` converts the `Message` into a `JsonMessage`, then serializes
that with `serde_json`:

| Field         | Type                     | Notes |
| ------------- | ------------------------ | ----- |
| `from`        | `Address \| null`         | `{ "name": ..., "address": ... }`, or `null` if unset. |
| `to`          | `Address[]`               | |
| `cc`          | `Address[]`               | |
| `bcc`         | `Address[]`               | |
| `subject`     | `string \| null`          | |
| `text`        | `string \| null`          | |
| `html`        | `string \| null`          | |
| `attachments` | `Attachment[]`            | Each entry is `{ "filename": ..., "content_type": ... }` — the resolved content type (guessed from the filename extension when not explicit), not the raw attachment bytes. |

An `Address` serializes as `{ "name": "...", "address": "..." }`, with `name` an empty
string when the original input had no display name (e.g. a bare `andris@nodemailer.com`
in a recipient list next to `Andris Kreata <andris@kreata.ee>` — both parse, and the
bare one gets `"name": ""`).

:::note[Deliberately simpler than nodemailer's JSON transport]
This schema does not include inline attachment content, raw headers, or an `envelope`
sub-object the way nodemailer's JSON transport does — it's a compact structural view of
the message, not a byte-exact serialization of everything nodemailer's version emits.
If your tests need to assert on attachment bytes or on generated headers, use the
[stream transport](/docs/transports/stream/) and inspect `raw` instead.
:::

## `SentInfo` shape

- `raw` — always `Some(bytes)`: the UTF-8 bytes of the JSON string above.
- `response` — always the fixed string `"250 json transport: message rendered, not
  sent"`, not the JSON payload itself — read `raw` (or call `render()` directly) to get
  the JSON.
- `accepted` / `envelope` — computed from `message.effective_envelope()`, same as every
  other transport.
- `rejected` — always empty.
- `message_id` — the `Message`'s own `message_id` field if you set one, `""` otherwise
  (this transport does not put a generated Message-ID anywhere in its JSON output at
  all, unlike the MIME-based transports, which always stamp one into the rendered
  headers even when `SentInfo.message_id` itself is empty).

## Not a stand-in for header-injection testing

:::caution[Does not exercise the CRLF-sanitizing MIME path]
The JSON transport's own test module says this explicitly: it serializes address/subject/
body fields straight into JSON strings, which is a completely different code path from
`mime::build()` — the function every real transport (SMTP, sendmail, SES) and the stream
transport all share, and the one place header-injection sanitization
(`mime/node.rs::write_header_line`, see [Security](/docs/security/)) actually lives. A
subject or address containing a CRLF will simply round-trip as a JSON string here; it
tells you nothing about whether that same input would be safely sanitized when actually
sent. If you're writing a regression test for header/address injection, use the
[stream transport](/docs/transports/stream/) (or a real transport) and assert on `raw`,
not this one.
:::

## Rust example

```rust
use trustmailer::{Attachment, Message};
use trustmailer::json::JsonTransport;

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let transport = JsonTransport::new();

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("Bob <bob@example.com>")
        .subject("Hello")
        .text("Hi Bob!")
        .attachment(
            Attachment::from_bytes("report.pdf", std::fs::read("report.pdf")?)
                .with_content_type("application/pdf"),
        )
        .build();

    let json = transport.render(&message)?;
    println!("{json}");
    // {"from":{"name":"Alice","address":"alice@example.com"},"to":[...],...}
    Ok(())
}
```

Like the stream transport, `render()` is the direct tool for tests; `Transport::send()`
wraps it with envelope computation and the fixed `SentInfo.response` string described
above.

## Node.js, Bun, Deno

`trustmailer-node`'s `createTransport()` exposes this transport with `jsonTransport:
true`.

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({ jsonTransport: true });

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});
console.log(info.response); // "250 json transport: message rendered, not sent"
```

:::caution[The Node/WASM bindings don't currently return the rendered JSON]
Same caveat as the stream transport: `SentInfoJs` (Node) and its WASM equivalent don't
expose a `raw` field, and `JsonTransport::send()` always sets `response` to the fixed
confirmation string rather than the JSON payload — so today, calling this transport
from Node.js, Bun, Deno, or a WASM host gets you delivery metadata, not the JSON
document itself. Call `render()` from Rust directly if you need the JSON.
:::

## See also

- [Stream transport](/docs/transports/stream/) — raw MIME rendering, and the transport
  to reach for if you specifically need to test header-injection safety.
- [Sendmail transport](/docs/transports/sendmail/) and [SES transport](/docs/transports/ses/)
  — the real delivery transports these testing transports stand in for.
- [Security](/docs/security/) — where and how header injection is actually prevented.
- [Runtime support matrix](/docs/runtime-matrix/).
