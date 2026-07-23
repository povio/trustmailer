---
title: Browser / WASM transport
description: Deep reference for trustmailer-wasm — what it supports in a browser tab, and why SMTP genuinely can't be one of those things.
---

`trustmailer-wasm` is the `wasm-bindgen` binding for hosts that are a genuine sandbox
with no native-code or raw-TCP escape hatch at all: a browser tab, first and foremost,
but the same compiled module also runs under Deno, Bun, and (if you don't need real
outbound SMTP) inside a Cloudflare Worker. For the fastest path to sending a message, see
the [browser/WASM quickstart](/docs/quickstart/browser-wasm/) first; this page covers
every configuration field, both supported backends in full, and the architectural reason
SMTP isn't — and can't be — a third option here.

## What's supported, and what deliberately isn't

`createTransport()` accepts exactly one of three `kind`s, confirmed directly from the
crate's `TransportOptions` enum:

- **`"stream"`** — renders the message to raw MIME bytes and returns them, without
  sending anything. You take it from there (a `fetch` call to your own backend, your own
  relay, wherever mail actually needs to go).
- **`"json"`** — same idea, but returns a JSON representation of the message instead of
  raw MIME.
- **`"ses"`** — sends via Amazon SES's HTTPS API. This is the one backend here that
  actually delivers mail on its own, because it's a plain HTTPS request — the exact same
  mechanism `fetch` uses, which `reqwest` (the HTTP client this crate is built on)
  transparently routes through on `wasm32`.

There is deliberately **no `"smtp"` kind**. This isn't an oversight or a missing feature
to file a ticket about:

:::caution[No raw TCP in a browser tab — for anyone, not just trustmailer]
A genuine browser tab has no raw-TCP or native-code API. `cloudflare:sockets` (used by
the separate [Cloudflare Workers transport](/docs/transports/cloudflare-workers/)) is a
`workerd`-specific built-in that simply doesn't exist outside an actual Worker isolate —
it isn't a browser API at all. No JavaScript mail library, trustmailer included, can open
an SMTP socket from a page loaded at a URL in a normal tab; the sandbox that makes a
browser tab safe to visit is exactly the thing that rules this out. See the
[runtime matrix](/docs/runtime-matrix/) for the full breakdown of what's architectural
versus what's a real gap elsewhere in the project.
:::

## Quick examples

### SES (actually sends mail)

```js
import init, { createTransport } from "trustmailer-wasm";

await init();

const transporter = createTransport({
  kind: "ses",
  region: "us-east-1",
  accessKeyId: "AKIA...",
  secretAccessKey: "...",
});

try {
  const info = await transporter.sendMail({
    from: "alice@example.com",
    to: "bob@example.com",
    replyTo: "support@example.com",
    subject: "Hello",
    text: "Hi Bob!",
    html: "<p>Hi Bob!</p>",
  });
  console.log(info.messageId, info.accepted, info.rejected);
} catch (err) {
  // A real Error — err.message, err.stack, `err instanceof Error` all work.
  console.error(err.message);
}
```

### Stream (render, then deliver yourself)

```js
import init, { createTransport } from "trustmailer-wasm";

await init();

const transporter = createTransport({ kind: "stream" });

const info = await transporter.sendMail({
  from: "alice@example.com",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});

// `info.response` holds the rendered raw MIME — hand it to your own backend/relay.
await fetch("/api/relay-outbound-mail", {
  method: "POST",
  headers: { "Content-Type": "text/plain" },
  body: info.response,
});
```

Swap `kind: "stream"` for `kind: "json"` to get a JSON representation back on `response`
instead of raw MIME text — same idea, different serialization for whatever your relay
expects.

## Configuration reference

`createTransport()`'s argument is a tagged object; unknown fields are rejected rather
than silently ignored:

| `kind`     | Field              | Required | Purpose |
| ---------- | ------------------- | :------: | ------- |
| any        | `kind`               | yes      | `"stream"`, `"json"`, or `"ses"`. |
| `"ses"`    | `region`             | yes      | AWS region, e.g. `"us-east-1"`. |
| `"ses"`    | `accessKeyId`        | yes      | AWS access key ID. |
| `"ses"`    | `secretAccessKey`    | yes      | AWS secret access key. |

`sendMail()`'s message object mirrors the same fields across all three backends:

| Field       | Type     | Notes |
| ----------- | -------- | ----- |
| `from`      | `string` | |
| `to`        | `string` | |
| `cc`        | `string` | |
| `bcc`       | `string` | |
| `replyTo`   | `string` | |
| `subject`   | `string` | |
| `text`      | `string` | |
| `html`      | `string` | |

There is currently no `attachments` field on this binding's `MailInput` — attachments
aren't yet reachable through the browser/WASM JS surface even though
`trustmailer-core::Message` fully supports them.

The response from `sendMail()` is a plain object (not a JS `Map` — see the callout
below) with `messageId`, `accepted`, `rejected`, and `response` fields.

:::caution[SES credentials in a browser tab]
Shipping a long-lived AWS secret access key to code running in someone's browser means
it's visible to that someone (and to anyone else who opens dev tools). The `"ses"` kind
is a straightforward, correct HTTPS integration — but for a real browser-facing product,
route it through a backend that holds the real credentials (or issues short-lived,
scoped ones) rather than embedding a long-lived secret key in client-side JavaScript. The
`"stream"`/`"json"` kinds plus your own authenticated backend endpoint are the safer
default shape for anything actually facing end users.
:::

## Fixed: the same-shaped `{}` response bug

An earlier version of both this crate and `trustmailer-smtp-cfworkers` returned `{}` for
every `sendMail()` call — not an error, just silently wrong data. The cause:
`serde-wasm-bindgen` serializes Rust struct fields as a JS `Map` by default, and a `Map`
has no own enumerable properties, so `JSON.stringify()`/`Response.json()` rendered it as
an empty object with no error raised anywhere. This was found by actually running the
Cloudflare Workers path live end to end and inspecting the real response — not by a
docs-only or compile-only review, which would never have surfaced it. The fix,
`serialize_maps_as_objects(true)`, is applied at every `sendMail()` call site in this
crate.

## Other hardening worth knowing about

- **Real `Error` objects, not bare thrown strings** — failures throw via `js_sys::Error`,
  so callers get a working `.stack` and `err instanceof Error` instead of an opaque
  string.
- **Unknown-field rejection** on both `TransportOptions` and `MailInput` — a typo'd field
  name is a hard error instead of being silently dropped.

## Composing with DKIM

At the Rust source level, `SigningTransport` (from `trustmailer-dkim`) is generic over
any `Transport` and has a dedicated inline signing path for `target_arch = "wasm32"`
(rather than `spawn_blocking`, since `wasm32` has no blocking thread pool and is
single-threaded anyway) — so `wasm32` support was clearly intended. In practice,
building `trustmailer-dkim` for `wasm32-unknown-unknown` currently runs into a known,
pre-existing gap in its dependency tree (the `rsa` crate's transitive `rand` →
`getrandom` dependency needs a WASM-entropy feature nothing in the graph currently
enables); `trustmailer-wasm` itself doesn't depend on `trustmailer-dkim`/`rsa` at all,
so it's unaffected by that gap. Separately, this crate's shipped `createTransport()` JS
binding doesn't currently expose a `dkim` option the way `trustmailer-node`'s does, so
DKIM signing isn't reachable from this JS surface out of the box today either way. See
[DKIM signing](/docs/dkim/) for the full picture.

See also: the [runtime matrix](/docs/runtime-matrix/) for the complete per-transport,
per-runtime breakdown, [Security](/docs/security/) for the full audit picture, and the
[browser/WASM quickstart](/docs/quickstart/browser-wasm/) for the minimal setup path.
