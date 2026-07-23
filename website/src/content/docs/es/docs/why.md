---
title: Por qué
description: nodemailer solo funciona donde funciona Node — por qué existe trustmailer.
---

Nodemailer es el estándar de facto para componer y enviar correo desde JavaScript, pero
solo funciona en Node — usa `net`/`tls`/`child_process`, nada de lo cual existe en una
pestaña de navegador o en un isolate de borde genuinamente aislado (Cloudflare Workers).
trustmailer conserva el mismo modelo mental — un `Message`, un `Transport` intercambiable,
`createTransport`/`sendMail` — pero lo implementa una sola vez en Rust y lo expone en
todas partes:

### Rust nativo

Crate `trustmailer`, `cargo add trustmailer`.

### Node.js, Bun, Deno

`trustmailer-node`, un addon nativo vía [napi-rs](https://napi.rs), con la misma forma de
API que `nodemailer.createTransport(...).sendMail(...)`. Node-API es un ABI estable, no
específico de Node: el mismo binario `.node` compilado corre sin modificaciones en Bun
(soporte N-API nativo) y en Deno (soporte N-API desde Deno 2.0, incluido Deno Deploy) —
verificado empíricamente en los tres, ver `crates/trustmailer-node/smoke-test.cjs`.

### Cloudflare Workers

`trustmailer-smtp-cfworkers`, SMTP real sobre la API `connect()` de `cloudflare:sockets`
(TCP saliente genuino, no un relay HTTP) — verificado contra una ejecución real de
`wrangler dev`/`workerd`, ver las páginas de [matriz de runtimes](/es/docs/runtime-matrix/)
y [seguridad](/es/docs/security/).

### Navegador / otros hosts WASM

`trustmailer-wasm`, vía `wasm-bindgen`, para hosts que son un sandbox genuino sin ninguna
vía de escape a código nativo ni a TCP crudo.

:::tip[Mismo contrato, cada runtime]
`Message`, `Transport`, `createTransport`, `sendMail` significan lo mismo sin importar a
qué crate estés llamando. Aprender la API de un runtime te enseña las cuatro.
:::
