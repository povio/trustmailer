---
title: "Guía rápida: Node.js, Bun, Deno"
description: Envía correo desde Node.js, Bun o Deno con el mismo addon nativo.
---

El mismo código en los tres runtimes — es el mismo addon nativo por debajo. Deno necesita
una extensión `.cjs`/`.mjs` o un `package.json` con `"type"` definido, según sus reglas de
detección de CJS.

```sh
cd crates/trustmailer-node
npx @napi-rs/cli build --platform --release
node smoke-test.cjs   # o: bun smoke-test.cjs / deno run -A smoke-test.cjs
```

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  host: "smtp.example.com",
  port: 587,
  auth: { user: "user", pass: "pass" },
  // dkim: { domainName: "example.com", keySelector: "default", privateKey: "-----BEGIN PRIVATE KEY-----..." },
});

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
  html: "<b>Hi Bob!</b>",
});
console.log("sent:", info.messageId);
```

:::note[ABI estable, no específico de Node]
Node-API es un ABI en C estable — el mismo binario `.node` compilado corre sin modificar
en Bun (soporte N-API nativo) y Deno (soporte N-API desde Deno 2.0, incluido Deno Deploy).
Verificado empíricamente en los tres en `crates/trustmailer-node/smoke-test.cjs`, no solo
asumido a partir de la especificación.
:::
