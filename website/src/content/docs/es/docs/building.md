---
title: Compilación
description: Comandos de build para cada target de trustmailer.
---

```sh
# workspace nativo (núcleo + todos los transportes nativos + fachada)
cargo build --workspace --exclude trustmailer-node

# addon nativo de Node.js/Bun/Deno (ejecutar desde crates/trustmailer-node) — un build, los tres runtimes
npx @napi-rs/cli build --platform --release
node smoke-test.cjs   # o: bun smoke-test.cjs / deno run -A smoke-test.cjs

# SMTP de Cloudflare Workers (ejecutar desde crates/trustmailer-smtp-cfworkers)
wasm-pack build --target web
# luego revisa test-worker/ para una reproducción completa con wrangler dev

# Navegador / otros hosts WASM (ejecutar desde crates/trustmailer-wasm)
wasm-pack build --target web
```

Ejecuta toda la suite de tests nativa con:

```sh
cargo test --workspace --exclude trustmailer-node --all-features
```
