---
title: Why
description: nodemailer only runs where Node runs — why trustmailer exists.
---

Nodemailer is the de facto standard for composing and sending mail from JavaScript, but
it's Node-only — it reaches into `net`/`tls`/`child_process`, none of which exist in a
browser tab or a true sandboxed edge isolate (Cloudflare Workers). trustmailer keeps the
same mental model — a `Message`, a pluggable `Transport`, `createTransport`/`sendMail` —
but implements it once in Rust and exposes it everywhere:

### Native Rust

`trustmailer` crate, `cargo add trustmailer`.

### Node.js, Bun, Deno

`trustmailer-node`, a native addon via [napi-rs](https://napi.rs), API-shaped like
`nodemailer.createTransport(...).sendMail(...)`. Node-API is a stable ABI, not a
Node-specific one: the exact same compiled `.node` binary runs unmodified under Bun
(native N-API support) and Deno (N-API support since Deno 2.0, including on Deno Deploy) —
verified empirically for all three, see `crates/trustmailer-node/smoke-test.cjs`.

### Cloudflare Workers

`trustmailer-smtp-cfworkers`, real SMTP over the `cloudflare:sockets` `connect()` API
(genuine outbound TCP, not an HTTP relay) — verified against a live `wrangler dev`/`workerd`
run, see the [runtime matrix](/docs/runtime-matrix/) and [security](/docs/security/) pages.

### Browser / other WASM hosts

`trustmailer-wasm`, via `wasm-bindgen`, for hosts that are a genuine sandbox with no
native-code or raw-TCP escape hatch at all.

:::tip[Same contract, every runtime]
`Message`, `Transport`, `createTransport`, `sendMail` mean the same thing regardless of
which crate you're calling into. Learning one runtime's API teaches you all four.
:::
