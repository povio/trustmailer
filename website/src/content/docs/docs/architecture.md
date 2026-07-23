---
title: Architecture
description: How trustmailer's crates fit together, and why the core compiles to wasm32 untouched.
---

```
trustmailer-core           message model, address parsing, MIME builder, Transport trait
  ├─ trustmailer-smtp-protocol  pure SMTP protocol logic (commands/responses/dot-stuffing) [any runtime]
  │    ├─ trustmailer-smtp        SMTP over tokio TCP (TLS/STARTTLS, auth, pool, proxy)    [native]
  │    └─ trustmailer-smtp-cfworkers  SMTP over cloudflare:sockets connect()               [Cloudflare Workers]
  ├─ trustmailer-sendmail   pipes to a local `sendmail` binary                          [native]
  ├─ trustmailer-ses        Amazon SES v2 HTTPS API, hand-rolled SigV4                  [any runtime]
  ├─ trustmailer-dkim       DKIM signing (RSA-SHA256, relaxed/relaxed)                  [any runtime]
  ├─ trustmailer-stream     renders raw MIME instead of sending (tests, custom relays)  [any runtime]
  └─ trustmailer-json       renders a JSON message instead of sending (tests)           [any runtime]

trustmailer              facade crate: re-exports core + all native transports, feature-gated
trustmailer-node         napi-rs binding — createTransport()/sendMail() for Node.js/Bun/Deno
trustmailer-wasm         wasm-bindgen binding — createTransport()/sendMail() for browser/other WASM hosts
```

`trustmailer-core` and `trustmailer-smtp-protocol` have no I/O and compile to `wasm32` as-is:
address parsing, MIME building, and SMTP command/response handling are pure functions over
bytes/strings, shared by every binding — only the byte-stream I/O (`tokio` TCP vs.
`cloudflare:sockets` vs. nothing at all) differs per transport.

:::note[Composable, not monolithic]
DKIM signing is a `Transport` decorator — `SigningTransport::new(signer, inner_transport)` —
so it wraps SMTP, SES, sendmail, or stream/JSON transparently, on any runtime that can build
`trustmailer-dkim`.
:::

See the [runtime support matrix](/docs/runtime-matrix/) for exactly which transport is
available on which runtime, and why.
