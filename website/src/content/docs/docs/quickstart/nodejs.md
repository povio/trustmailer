---
title: "Quickstart: Node.js, Bun, Deno"
description: Send mail from Node.js, Bun, or Deno with the same native addon.
---

Identical code on all three runtimes — it's the same native addon underneath. Deno needs
a `.cjs`/`.mjs` extension or a `package.json` with `"type"` set, per its CJS-detection rules.

```sh
cd crates/trustmailer-node
npx @napi-rs/cli build --platform --release
node smoke-test.cjs   # or: bun smoke-test.cjs / deno run -A smoke-test.cjs
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

:::note[Stable ABI, not a Node-specific one]
Node-API is a stable C ABI — the exact same compiled `.node` binary runs unmodified under
Bun (native N-API support) and Deno (N-API support since Deno 2.0, including on Deno
Deploy). Verified empirically for all three in `crates/trustmailer-node/smoke-test.cjs`,
not just assumed from the spec.
:::
