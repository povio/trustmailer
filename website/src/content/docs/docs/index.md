---
title: Documentation
description: trustmailer docs — a nodemailer-compatible mail library, Rust-powered, for native Rust, Node.js/Bun/Deno, Cloudflare Workers, and the browser.
tableOfContents: false
---

A [nodemailer](https://nodemailer.com)-compatible mail library, Rust-powered, for every
runtime: native Rust, Node.js/Bun/Deno (one native addon, no per-runtime rebuild),
Cloudflare Workers (real SMTP over genuine outbound TCP, not an HTTP relay), and the
browser/other WASM hosts. Same message model, same transport contract — one Rust core,
four ways to call it.

`trust` because it's Rust, and because you shouldn't have to trust a JS mail stack's
transitive dependency tree with your SMTP credentials.

## Where to start

- **[Why](/docs/why/)** — the problem trustmailer solves, and the four crates that solve it.
- **[Architecture](/docs/architecture/)** — how the crates fit together, and why the core compiles to `wasm32` untouched.
- **Quickstart** — copy-paste `createTransport`/`sendMail` for [native Rust](/docs/quickstart/native-rust/), [Node/Bun/Deno](/docs/quickstart/nodejs/), [Cloudflare Workers](/docs/quickstart/cloudflare-workers/), and the [browser](/docs/quickstart/browser-wasm/).
- **[Runtime support matrix](/docs/runtime-matrix/)** — exactly which transport works on which runtime, and why the gaps are architectural, not missing features.
- **[Security](/docs/security/)** — what was found, what was fixed, and how it was verified.
- **[Building](/docs/building/)** — build commands for every target.

Source, issues, and the full audit log live on [GitHub](https://github.com/nesgarbo/trustmailer).
