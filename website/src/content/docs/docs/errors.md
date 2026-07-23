---
title: Errors
description: Every trustmailer error variant, what triggers it, and how it surfaces in Rust and Node.js.
---

Nodemailer surfaces failures as an `Error` object with a string `.code` (`"EENVELOPE"`,
`"EAUTH"`, `"ECONNECTION"`, …) that you match on. trustmailer uses a typed Rust enum
instead — `trustmailer_core::Error`, defined with [`thiserror`](https://docs.rs/thiserror)
— so failures are exhaustively matchable at compile time, not string comparisons that can
silently drift out of sync with the library.

```rust
pub enum Error {
    InvalidAddress(String),
    NoRecipients,
    NoSender,
    Attachment(String),
    Mime(String),
    Transport(String),
    Io(std::io::Error),
    Dkim(String),
}
```

Every fallible trustmailer call — `Message::builder()...build()`'s downstream
`transport.send(&message)`, `Attachment::resolve(...)`, `DkimSigner::from_pkcs8_pem(...)`
— returns `trustmailer::Result<T>` (an alias for `Result<T, Error>`), so `?` and `match`
work exactly like any other Rust error type.

## Variants

### `InvalidAddress(String)`

An address string couldn't be resolved to anything usable. In practice this is rare — the
[address parser](/docs/addresses/) is deliberately permissive/infallible for free-text
input (nodemailer's own design: better a best-effort guess than a hard failure on a messy
`To:` line) — this variant exists for the narrower cases where a caller-supplied structured
`Address` is unusable regardless (e.g. empty after trimming).

### `NoRecipients`

Message body text: `"no recipients defined (to/cc/bcc/envelope.to)"`.

Raised by [`Message::effective_envelope()`](/docs/message/#envelope-and-effective_envelope)
when `to`, `cc`, `bcc`, and an explicit `envelope` override are *all* empty — there's
nowhere for the SMTP transaction to send `RCPT TO`. Fix: set at least one of `to`/`cc`/`bcc`,
or call `.envelope(...)` directly.

### `NoSender`

Message body text: `"no sender defined (from/sender/envelope.from)"`.

The symmetric counterpart to `NoRecipients` — raised when `from`, `sender`, and an explicit
`envelope` override are all empty, so `MAIL FROM` has nothing to send. Fix: call
`.from(...)` (or `.envelope(...)`) before `.build()`.

:::note[Added in the second security audit]
This variant didn't exist in the first pass — a caller could previously get an
envelope with an empty `from` and no error at all. See [Security](/docs/security/) for the
full story of what the independent re-audit found and fixed.
:::

### `Attachment(String)`

Wraps everything that can go wrong resolving an [attachment's](/docs/attachments/) content
into bytes:

- `AttachmentContent::Base64` with content that isn't valid base64.
- `AttachmentContent::Url` when the HTTP fetch itself fails (DNS, connection refused, TLS
  error, non-2xx status).
- `AttachmentContent::Url` when the response exceeds trustmailer-core's 25MB
  `MAX_FETCH_URL_BYTES` cap — checked against `Content-Length` up front *and* enforced
  while streaming, so a server lying about its `Content-Length` can't bypass it.
- `AttachmentContent::Url` when reading the response body itself fails partway through.

### `Mime(String)`

Rare in practice today — the only current source is `trustmailer-json`'s transport
failing to serialize a `Message` to JSON (an internal `serde_json` error, not something
caller input normally triggers). Reserved for MIME-build-time failures more broadly.

### `Transport(String)`

The catch-all for delivery-layer failures across every transport: SMTP connect/handshake/
auth/protocol errors (see the [SMTP transport](/docs/transports/smtp/) page for the many
specific conditions that map here — STARTTLS refused, AUTH mechanism not advertised,
SOCKS5 proxy misconfigured, OAuth2 token fetch failed, connect/command timeouts), sendmail
process spawn/exit failures, and SES/HTTP-API non-2xx responses. If you need to
distinguish *which* transport failed, match on the transport-specific error text embedded
in the `String` — there's no further sub-typing today.

### `Io(std::io::Error)`

Wraps a raw `std::io::Error` via `#[from]` — mostly filesystem I/O for
`AttachmentContent::Path` (behind the `fs` feature) and any lower-level socket I/O that
surfaces as `io::Error` rather than being wrapped into a `Transport` message first.

### `Dkim(String)`

Everything DKIM-specific, from [`DkimSigner`](/docs/dkim/):

- `domain`/`selector` containing `\r`, `\n`, `;`, or `"` — rejected at construction, before
  any header is ever built, closing the header-injection path a first audit pass found.
- An invalid PKCS#8 private key passed to `from_pkcs8_pem`.
- No signable headers found on the message at all.
- **`From` header must be signed** — DKIM requires it per RFC 6376; if `.with_headers(...)`
  excludes `"from"` (or the message has no `From` at all), signing refuses rather than
  emitting a signature that doesn't cover the sender identity.
- The RSA signing operation itself failing (essentially never, barring a corrupt key).

## Matching in Rust

```rust
use trustmailer::Error;

match transport.send(&message).await {
    Ok(info) => println!("sent: {}", info.message_id),
    Err(Error::NoRecipients) => eprintln!("nothing to send to — check to/cc/bcc"),
    Err(Error::Transport(msg)) => eprintln!("delivery failed: {msg}"),
    Err(e) => eprintln!("send failed: {e}"),
}
```

## In Node.js

`trustmailer-node` maps every `Error` variant to a `napi::Error` with a `[VariantName]`
prefix on the message and a `Status` chosen per variant — verified from
`crates/trustmailer-node/src/lib.rs`'s `to_napi_err`:

| Variant | napi `Status` | Message prefix |
| --- | --- | --- |
| `InvalidAddress` | `InvalidArg` | `[InvalidAddress]` |
| `NoRecipients` | `InvalidArg` | `[NoRecipients]` |
| `NoSender` | `InvalidArg` | `[NoSender]` |
| `Attachment` | `InvalidArg` | `[Attachment]` |
| `Mime` | `GenericFailure` | `[Mime]` |
| `Transport` | `GenericFailure` | `[Transport]` |
| `Io` | `GenericFailure` | `[Io]` |
| `Dkim` | `InvalidArg` | `[Dkim]` |

```js
try {
  await transporter.sendMail({ subject: "Hello", text: "Hi!" }); // no to/from
} catch (err) {
  if (err.message.startsWith("[NoRecipients]")) {
    console.error("set at least one of to/cc/bcc");
  }
  throw err;
}
```

:::tip[No string error codes to keep in sync]
Nodemailer's `err.code` strings (`"EENVELOPE"`, `"EAUTH"`, …) live only in its
documentation — there's no compiler check that your `if (err.code === "EAUTH")` still
matches a real code after an upgrade. trustmailer's Rust `match` on `Error` is exhaustive
and compiler-checked; the Node.js `[VariantName]` prefix is the closest equivalent on that
side, generated directly from the same enum rather than hand-maintained separately.
:::
