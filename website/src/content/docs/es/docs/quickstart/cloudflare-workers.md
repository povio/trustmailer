---
title: "Guía rápida: Cloudflare Workers"
description: SMTP real desde un Cloudflare Worker sobre TCP saliente genuino.
---

Compila con `wasm-pack build --target web`, importa el `.wasm` generado como módulo (el
bundler de Wrangler lo convierte en un `WebAssembly.Module`), y usa `initSync` — no la
exportación `init()` async por defecto, que necesita un `await` de nivel superior.

```sh
cd crates/trustmailer-smtp-cfworkers
wasm-pack build --target web
# luego revisa test-worker/ para una reproducción completa con wrangler dev
```

```js
import wasmModule from "./pkg/trustmailer_smtp_cfworkers_bg.wasm";
import { initSync, createTransport } from "./pkg/trustmailer_smtp_cfworkers.js";

initSync({ module: wasmModule });

export default {
  async fetch() {
    const transporter = createTransport({
      host: "smtp.gmail.com",
      port: 465,
      secure: true,
      auth: { user: "user@gmail.com", pass: "app-password" },
    });
    const info = await transporter.sendMail({
      from: "Alice <alice@example.com>",
      to: "bob@example.com",
      subject: "Hello from a Worker",
      text: "Sent over a real TCP connection.",
    });
    return Response.json(info);
  },
};
```

## Esto es SMTP real, no un relay — compruébalo tú mismo

Los Workers exponen una API de TCP saliente genuina, `connect()` de `cloudflare:sockets`,
con la plataforma manejando el TLS por ti (`secureTransport: "on"` para TLS implícito,
`"starttls"` + `.startTls()` para STARTTLS). La única restricción es el puerto 25
(relay MX-a-MX, bloqueado — igual que en cualquier proveedor cloud grande, para frenar el
spam); los puertos 587/465 (envío SMTP, lo que `SmtpConfig` usa por defecto) no tienen
restricción.

Reproduce la verificación en vivo tú mismo:

```sh
cd crates/trustmailer-smtp-cfworkers
wasm-pack build --target web --out-dir test-worker/pkg
cd test-worker
node fake-smtp-server.cjs 2525 &
npx wrangler dev --local
curl "http://127.0.0.1:8787/?host=127.0.0.1&port=2525"
# {"ok":true,"info":{"accepted":["bob@example.com"],"response":"OK: queued as 12345", ...}}
```

:::caution[Una limitación más estrecha, pero genuina]
Una pestaña de navegador real no tiene ninguna API de TCP crudo ni de código nativo —
ninguna librería de correo en JS puede hacer SMTP desde una página web tampoco. Para ese
host, consulta la [guía rápida de navegador/WASM](/es/docs/quickstart/browser-wasm/).
:::

Consulta la [referencia del transporte Cloudflare Workers](/es/docs/transports/cloudflare-workers/)
para las tablas de opciones completas, el desglose de modos TLS, y qué está verificado en
vivo frente a lo que solo se revisó en el código.
