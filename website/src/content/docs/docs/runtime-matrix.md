---
title: Runtime support matrix
description: Exactly which transport works on which runtime, and why.
---

| Transport            | Native Rust | Node · Bun · Deno | Cloudflare Workers                                    | Browser / other WASM                        |
| --------------------- | :---------: | :----------------: | :------------------------------------------------------ | :-------------------------------------------- |
| SMTP (TLS/STARTTLS)   | ✅          | ✅                  | ✅ — real TCP via `cloudflare:sockets`, not a relay      | ❌¹ — no raw TCP sockets in a normal web page |
| Sendmail              | ✅          | ✅                  | ❌² — no process spawning in the Worker isolate          | ❌ — no process spawning, full stop           |
| SES (HTTPS API)       | ✅          | ✅                  | ✅                                                        | ✅ — `reqwest` uses `fetch` under `wasm32`    |
| Stream / JSON         | ✅          | ✅                  | ✅                                                        | ✅                                             |
| DKIM signing          | ✅ (`SigningTransport` wraps any transport) | ✅ (`dkim` option on `createTransport`) | ✅ (compose with `trustmailer-dkim` directly) | ✅ |

¹ Checked, not assumed: Chrome does have a raw-TCP
[Direct Sockets API](https://developer.chrome.com/docs/iwa/direct-sockets), but it's
scoped to **Isolated Web Apps** — a separately packaged, signed, installed app model, not
a page loaded at a URL in a normal tab — so it doesn't apply to "browser" in the sense this
crate targets (a library a web page imports). No other browser ships anything equivalent.

² Checked, not assumed: the Worker isolate genuinely cannot spawn a process — Cloudflare's
own docs are explicit that Workers run on a V8 isolate with no disk and no way to spawn a
native process. [Cloudflare Containers](https://developers.cloudflare.com/containers/) (a
separate product — real Linux containers, with `exec()` support) *can* run an actual
`sendmail` binary, but that's a different compute primitive, not something
`trustmailer-smtp-cfworkers`'s Worker-isolate code can touch.

:::tip[Go verify it yourself]
Cloudflare Workers get real SMTP, not a relay — the reproduction steps are on the
[Cloudflare Workers quickstart](/docs/quickstart/cloudflare-workers/) page. A real
`wrangler dev` project sends a full MIME message over a real TCP SMTP transaction to a
local fake SMTP server.
:::

Both gaps above are architectural, not missing features — see [Security](/docs/security/)
for how the rest of the transport surface is tested and verified.
