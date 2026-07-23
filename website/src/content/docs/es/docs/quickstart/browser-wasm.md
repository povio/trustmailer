---
title: "Guía rápida: Navegador / otro WASM"
description: Envía correo desde una pestaña de navegador o cualquier host WASM vía SES o stream/JSON.
---

```sh
cd crates/trustmailer-wasm
wasm-pack build --target web
```

```js
import init, { createTransport } from "trustmailer-wasm";

await init();
const transporter = createTransport({
  kind: "ses",
  region: "us-east-1",
  accessKeyId: "...",
  secretAccessKey: "...",
});

const info = await transporter.sendMail({
  from: "alice@example.com",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});
```

:::caution[Sin TCP crudo en una pestaña de navegador]
Una pestaña de navegador real no tiene ninguna API de TCP crudo ni de código nativo — esto
no es una limitación de trustmailer, ninguna librería de correo en JS puede abrir un
socket SMTP desde una página web normal tampoco. Usa el transporte **SES** (una simple
llamada HTTPS, funciona donde sea que funcione `fetch`) o el transporte **stream/JSON**
para renderizar el mensaje MIME y entregarlo a tu propio relay/API vía `fetch`.
:::

Consulta la [referencia del transporte navegador/WASM](/es/docs/transports/browser-wasm/)
para las tablas de opciones completas y cada `kind` soportado.
