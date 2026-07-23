---
title: "Quickstart: Browser / other WASM"
description: Send mail from a browser tab or any WASM host via SES or stream/JSON.
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

:::caution[No raw TCP in a browser tab]
A genuine browser tab has no raw-TCP or native-code API — this isn't a trustmailer gap,
no JS mail library can open an SMTP socket from a normal web page either. Use the **SES**
transport (a plain HTTPS call, works everywhere `fetch` does) or the **stream/JSON**
transport to render the MIME message and hand it to your own relay/API over `fetch`.
:::

See the [browser/WASM transport reference](/docs/transports/browser-wasm/) for the full
option tables and every supported `kind`.
