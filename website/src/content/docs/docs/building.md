---
title: Building
description: Build commands for every trustmailer target — native Rust, the Node.js addon, Cloudflare Workers, and browser/WASM.
---

```sh
# native workspace (core + all native transports + facade)
cargo build --workspace --exclude trustmailer-node

# Node.js/Bun/Deno native addon (run from crates/trustmailer-node) — one build, all three runtimes
npx @napi-rs/cli build --platform --release
node smoke-test.cjs   # or: bun smoke-test.cjs / deno run -A smoke-test.cjs

# Cloudflare Workers SMTP (run from crates/trustmailer-smtp-cfworkers)
wasm-pack build --target web
# then see test-worker/ for a full wrangler dev reproduction against a local fake SMTP server

# Browser / other WASM hosts (run from crates/trustmailer-wasm)
wasm-pack build --target web
```

Run the full native test suite with:

```sh
cargo test --workspace --exclude trustmailer-node --all-features
```
