---
title: "Quickstart: Cloudflare Workers"
description: Real SMTP from a Cloudflare Worker over genuine outbound TCP.
---

Build with `wasm-pack build --target web`, import the generated `.wasm` file as a module
(Wrangler's bundler turns it into a `WebAssembly.Module`), and `initSync` — not the default
async `init()` export, which needs a top-level `await`.

```sh
cd crates/trustmailer-smtp-cfworkers
wasm-pack build --target web
# then see test-worker/ for a full wrangler dev reproduction against a local fake SMTP server
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

## This is real SMTP, not a relay — go verify it

Workers ship a genuine outbound-TCP API, `cloudflare:sockets`'s `connect()`, with the
platform handling TLS for you (`secureTransport: "on"` for implicit TLS, `"starttls"` +
`.startTls()` for STARTTLS). The only restriction is port 25 (MX-to-MX relay, blocked —
same as every major cloud provider, to fight spam); ports 587/465 (SMTP *submission*, what
`SmtpConfig` defaults to) are unrestricted.

Reproduce the live verification yourself:

```sh
cd crates/trustmailer-smtp-cfworkers
wasm-pack build --target web --out-dir test-worker/pkg
cd test-worker
node fake-smtp-server.cjs 2525 &
npx wrangler dev --local
curl "http://127.0.0.1:8787/?host=127.0.0.1&port=2525"
# {"ok":true,"info":{"accepted":["bob@example.com"],"response":"OK: queued as 12345", ...}}
```

:::caution[Narrower, genuine limitation]
An actual browser tab has no raw-TCP or native-code API at all — no JS mail library can do
SMTP from a web page either. For that host, see the [browser/WASM quickstart](/docs/quickstart/browser-wasm/).
:::

See the [Cloudflare Workers transport reference](/docs/transports/cloudflare-workers/) for
the full option tables, the TLS-mode breakdown, and what's live-verified versus code-reviewed.
