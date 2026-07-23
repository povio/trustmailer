---
title: Sendmail transport
description: Piping composed mail into a local sendmail-compatible binary's stdin — no shell, no network.
---

The sendmail transport hands your composed message to a local `sendmail`-compatible
binary instead of talking SMTP itself. This is the classic Unix delivery pattern: a mail
transfer agent (Postfix, Exim, OpenSMTPD, msmtp...) installs a `/usr/sbin/sendmail`
shim, and anything that writes an RFC 5322 message to that shim's stdin gets local
queuing, relaying, and retry handling for free. It's implemented in
`trustmailer-sendmail`, re-exported from the facade crate as `trustmailer::sendmail`
behind the `sendmail` Cargo feature (on by default).

This transport spawns a real OS process, so it only exists on native runtimes — it is
not available from `trustmailer-wasm` (browsers and Cloudflare Workers have no ability
to spawn processes) and building `trustmailer` for `wasm32-unknown-unknown` with the
`sendmail` feature enabled fails at compile time on purpose. See the
[runtime support matrix](/docs/runtime-matrix/) for the full picture.

## Configuration

`SendmailTransport` is built with `SendmailTransport::new()` and two optional builder
methods:

| Field  | Type          | Default                        | Description |
| ------ | ------------- | ------------------------------- | ----------- |
| `path` | `String`      | `/usr/sbin/sendmail`             | Path to the sendmail-compatible binary, set via `.with_path(path)`. Should generally be absolute — a relative value falls back to a `$PATH` lookup, which is rarely what you want in production. |
| `args` | `Vec<String>` | `["-i", "-t"]`                   | Full command-line argument list passed to the binary, set via `.with_args(args)`. **Replaces** the default `-i -t` entirely rather than appending to it — if you call `.with_args(...)`, you're responsible for including `-t` yourself if you still want recipients read from the message headers. |

`-i` tells sendmail not to treat a lone `.` on a line as end-of-input, and `-t` tells it
to take recipients from the message's `To`/`Cc`/`Bcc` headers rather than from argv.
Both are conventions, not something trustmailer enforces — since `.with_args()`
replaces the list outright, you can pass whatever argv your local MTA expects (for
example `-f` to force a distinct envelope-sender address).

:::caution[`path`/`args` are operator config, not attacker input]
`path` and `args` are supplied by whoever configures the transport in code — they are
never derived from message content, sender/recipient addresses, or anything else an
end user controls. Don't build them from untrusted input; treat them the same as a
database connection string or an API endpoint URL.
:::

## How it sends

`SendmailTransport::send()`:

1. Renders the message to raw RFC 5322 bytes via the same `mime::build()` every other
   transport uses, with the Message-ID hostname hard-coded to `localhost` (there is no
   `with_hostname()` on this transport the way there is on the [stream
   transport](/docs/transports/stream/) — if you need a specific Message-ID domain, set
   `.message_id(...)` explicitly on the `Message`).
2. Spawns `path` with `args` via `tokio::process::Command`, with stdin, stdout, and
   stderr all piped. This is a pure argv `Command::new(path).args(args)` call — there is
   no shell in the middle, so there's no shell-metacharacter injection surface even in
   principle.
3. Writes the rendered message to the child's stdin and closes it (so the child sees
   EOF), while **concurrently** draining stdout and stderr.
4. Waits for the process to exit. A non-zero exit status is reported as an error that
   includes the captured stderr text.

Step 3's concurrency is not incidental. An earlier version of this transport wrote all
of stdin to completion first and only *then* read stdout/stderr (via
`wait_with_output()`-style sequencing). A sendmail binary — or a verbose wrapper around
one — can fill the OS pipe buffer with stdout/stderr output before it has drained a
large stdin body (a message with sizeable attachments, say), and writing-then-reading
in strict sequence can deadlock both sides forever, with no timeout. The fix writes
stdin and reads stdout/stderr in the same `tokio::join!`, so neither side can block the
other.

:::note[A fixed pipe-deadlock class of bug]
If you're evaluating trustmailer against other sendmail-shelling implementations, this
is exactly the kind of bug to check for: does it read child output *while* writing
stdin, or only after? Sequential write-then-read is correct for small messages and a
quiet sendmail, and silently hangs for large ones. `trustmailer-sendmail`'s test suite
spawns real `/bin/cat` and `/bin/sh` processes (not mocks) as sendmail stand-ins to
exercise this.
:::

## `SentInfo` shape

- `accepted` — every envelope recipient (`message.effective_envelope()?.to`). Sendmail
  gives no way to distinguish "accepted" from "rejected" per recipient at the pipe
  boundary, so this transport can't populate `rejected` — it's always empty here; a
  delivery problem shows up as a non-zero exit and an `Err`, not as a rejected list.
- `response` — whatever the binary printed to stdout, usually nothing (real sendmail
  binaries are typically silent on success).
- `raw` — always `None`. The rendered message was piped directly into the child's
  stdin and never retained in memory afterward, unlike the stream/JSON transports.
- `message_id` — the `Message`'s own `message_id` field if you set one, `""` otherwise.
  This is a real subtlety: the *rendered* MIME always gets a generated `Message-ID`
  header (via step 1 above) even when you didn't set one, but `SentInfo.message_id`
  only reflects what was on the `Message` struct going in. If you want a non-empty
  `message_id` back, call `.message_id(...)` on the builder yourself.

## Rust example

```rust
use trustmailer::{Message, Transport};
use trustmailer::sendmail::SendmailTransport;

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let transport = SendmailTransport::new()
        .with_path("/usr/sbin/sendmail")
        .with_args(vec!["-i".into(), "-t".into(), "-f".into(), "bounces@example.com".into()]);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .message_id("<unique-id@example.com>")
        .build();

    let info = transport.send(&message).await?;
    println!("accepted: {:?}", info.accepted);
    Ok(())
}
```

## Node.js, Bun, Deno

`trustmailer-node`'s `createTransport()` exposes this transport with `sendmail: true`
and an optional `path`. There is currently no way to override `args` from the Node
binding — that builder method exists only on the Rust `SendmailTransport` itself, so if
you need custom argv, call the Rust crate directly or open an issue.

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  sendmail: true,
  path: "/usr/sbin/sendmail", // optional, this is the default
});

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});
console.log("accepted:", info.accepted);
```

Unlike calling the Rust crate directly, `info.messageId` here is never empty: the Node
binding pre-generates a `Message-ID` before calling into the transport whenever the
caller didn't supply one, so `SentInfoJs.message_id` always reflects a real ID.

## See also

- [SES transport](/docs/transports/ses/) — HTTPS API delivery instead of a local binary.
- [Stream transport](/docs/transports/stream/) and [JSON transport](/docs/transports/json/)
  — no delivery at all, for tests and custom relays.
- [Runtime support matrix](/docs/runtime-matrix/) — why this transport is native-only.
- [Security](/docs/security/) — how header/address injection is prevented before the
  message ever reaches this transport.
