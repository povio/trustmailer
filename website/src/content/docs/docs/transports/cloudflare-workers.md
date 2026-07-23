---
title: Cloudflare Workers transport
description: Deep reference for trustmailer-smtp-cfworkers — real outbound TCP SMTP inside a Worker isolate.
---

`trustmailer-smtp-cfworkers` is a full SMTP client transport built specifically for
running **inside a Cloudflare Worker**. It speaks the real SMTP protocol over the
`cloudflare:sockets` module's `connect()` API — genuine outbound TCP, with the platform
handling TLS for you — rather than proxying mail through an HTTP API. If you just want
the fastest path to sending a message, see the
[Cloudflare Workers quickstart](/docs/quickstart/cloudflare-workers/) first; this page is
the deeper reference for everything that quickstart doesn't cover: every configuration
field, both TLS modes, the auth mechanisms, and exactly what's been verified against a
real `workerd` run versus what's only been checked at the config level.

## Why this needs its own crate

`cloudflare:sockets` is a `workerd` built-in module. It only resolves inside the actual
Cloudflare Workers runtime — not in a browser tab, not under Deno or Bun running the
same compiled `.wasm` directly, not under `wasm-pack test`. Outside of a real Worker,
`connect()` is simply undefined. That's why this is a separate crate from
`trustmailer-wasm` rather than another `kind` option there: it's the one transport whose
target environment is narrower than "anything wasm32 can run."

The protocol logic itself — command formatting, response-line parsing, dot-stuffing,
AUTH encoding — lives in the shared `trustmailer-smtp-protocol` crate and is exactly the
same code the native `trustmailer-smtp` transport uses. Only the byte-stream I/O layer
differs: this crate drives `cloudflare:sockets`' Web Streams reader/writer instead of a
`tokio::net::TcpStream`.

## Quick example

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
      auth: {
        type: "login",
        user: "user@gmail.com",
        pass: "app-password",
      },
    });

    try {
      const info = await transporter.sendMail({
        from: "Alice <alice@example.com>",
        to: "bob@example.com",
        replyTo: "support@example.com",
        subject: "Hello from a Worker",
        text: "Sent over a real TCP connection.",
      });
      return Response.json(info);
    } catch (err) {
      // Thrown as a real Error — err.message, err.stack, `err instanceof Error` all work.
      return new Response(err.message, { status: 502 });
    }
  },
};
```

## Configuration reference

`createTransport()` takes a plain object matching this shape (unknown fields are
rejected outright, not silently ignored):

| Field         | Type                                  | Default        | Purpose |
| ------------- | -------------------------------------- | --------------- | ------- |
| `host`        | `string`                               | — (required)    | SMTP server hostname. |
| `port`        | `number`                               | — (required)    | SMTP server port. |
| `secure`      | `boolean`                              | `port === 465`  | `true` for implicit TLS (typically port 465) — same port-based default as the native SMTP transport, unless you set it explicitly. See [TLS modes](#tls-modes) below. |
| `requireTLS`  | `boolean`                              | `true`          | When `secure` is `false`, whether the connection must upgrade via STARTTLS before proceeding. Only turn this off against a trusted plaintext server (e.g. local testing) — see the caution below. |
| `auth`        | `{ type?, user, pass }`                | none (no AUTH)  | Credentials. `type` is `"plain"` (default when omitted), `"login"`, or `"xoauth2"`; `user`/`pass` are used as username/password for `plain`/`login`, or username/access-token for `xoauth2`. |
| `name`        | `string`                               | `"localhost"`   | The name sent in the `EHLO` command. Any CR/LF is stripped before use. |

The Rust-side config (`CfSmtpConfig`, if you're writing your own `wasm-bindgen` glue
around the crate directly instead of going through `js.rs`) exposes the same knobs as
builder methods: `CfSmtpConfig::new(host, port)`, `.with_secure(bool)`,
`.with_auth(Auth::Plain | Auth::Login | Auth::XOAuth2 { .. })`,
`.with_ehlo_name(String)`, `.with_require_tls(bool)`.

## TLS modes

| `secure` | `requireTLS` | What happens |
| :------: | :----------: | ------------ |
| `true`   | *(ignored)*  | Implicit TLS: `cloudflare:sockets` opens the socket with `secureTransport: "on"`, so the whole session — including the initial greeting — is encrypted from the first byte. Matches port-465-style servers. |
| `false`  | `true` *(default)* | Connects in the clear, sends `EHLO`, then requires the server to advertise `STARTTLS` and issues it, upgrading via `secureTransport: "starttls"` + the socket's `.startTls()` before any credentials or mail data are sent. If the server doesn't advertise `STARTTLS`, the connection is refused with an error rather than silently falling back to plaintext. Matches port-587-style servers. |
| `false`  | `false`      | Plaintext for the entire session, no upgrade attempted. Only meant for a trusted local/test server — see the caution below. |

:::caution[`requireTLS: false` is a testing knob, not a production setting]
Turning off `requireTLS` on a non-`secure` connection means credentials and mail content
travel unencrypted for the whole session. It exists so the crate's own integration tests
can run against a local fake SMTP server without a certificate — it isn't meant to be
set against anything reachable over a real network.
:::

## Authentication

`auth.type` selects which SASL mechanism is used, mapping onto the shared
`trustmailer-smtp-protocol::Auth` enum:

- **`"plain"`** (default if `type` is omitted) — single `AUTH PLAIN` exchange.
- **`"login"`** — the classic `AUTH LOGIN` username/password prompt-response sequence.
- **`"xoauth2"`** — `user` is the account, `pass` is treated as the OAuth2 access token.

Before attempting AUTH, the transport checks that the server actually advertised an
`AUTH` capability in its `EHLO` response and fails with a clear error if it didn't,
rather than sending credentials into the void and reporting a confusing protocol error
back.

## This is real SMTP, not a relay

There's no HTTP-to-SMTP gateway anywhere in this path. `cloudflare:sockets`'
`connect()` opens a genuine outbound TCP socket from the Worker isolate; this crate
speaks real `EHLO`/`MAIL FROM`/`RCPT TO`/`DATA` on top of it, with the platform handling
the TLS handshake. The only network-level restriction Cloudflare imposes is blocking
outbound port 25 (MX-to-MX relay, blocked the same way on every major cloud provider, to
fight spam) — SMTP *submission* on 587/465, which is what this transport and everyday
"send mail via SMTP" both actually mean, is unrestricted.

You can reproduce the live verification yourself:

```sh
cd crates/trustmailer-smtp-cfworkers
wasm-pack build --target web --out-dir test-worker/pkg
cd test-worker
node fake-smtp-server.cjs 2525 &
npx wrangler dev --local
curl "http://127.0.0.1:8787/?host=127.0.0.1&port=2525"
# {"ok":true,"info":{"accepted":["bob@example.com"],"response":"OK: queued as 12345", ...}}
```

:::note[Precisely what "live-verified" covers today]
The reproduction above — and the audit's live run against real `workerd` — exercises the
**plaintext path** (`secure: false, requireTLS: false`) against a local fake SMTP server.
The `secure: true` (implicit TLS) and STARTTLS code paths are the security-critical ones,
and as of the last audit pass they've been verified by careful code inspection (correct
sequencing, no silent downgrade, the pre-TLS read buffer is properly discarded across the
upgrade) and by config-level unit tests, but not yet exercised against a real
TLS-terminating `cloudflare:sockets` connection end to end. That's a meaningfully weaker
verification claim than the plaintext path's, and it's called out here rather than
folded into a blanket "fully verified."
:::

## Fixed: the socket-leak-on-error path

An earlier version of this transport only ran `QUIT`/`close()` on the success path.
Any error partway through a transaction — a rejected recipient, a failed `AUTH`, a
`STARTTLS` that didn't come back clean — returned early via `?` before cleanup ever ran,
leaking the underlying TCP socket. `CfSocket` can't fix this with a `Drop` impl, because
real cleanup is `async` (closing the socket means awaiting a promise) and Rust's `Drop`
is synchronous. The actual fix is a `close_after` helper that both `send()` and `verify()`
route through unconditionally — `QUIT` and `close()` now run on *every* path, success or
failure, before the original result (or error) is returned to the caller. Left over a
long-lived Worker isolate, the old behavior would have accumulated unclosed sockets
against Cloudflare's per-isolate connection limit; this closes that gap.

## Other hardening worth knowing about

- **8KB cap on a single buffered response line.** `read_line` fails fast with a clear
  error instead of growing its internal buffer without bound if a server (or a
  misbehaving proxy) never sends a line terminator — protecting against an
  out-of-memory/isolate-crash vector in a Worker's capped memory budget.
- **Real `Error` objects, not bare thrown strings.** Failures surface via
  `js_sys::Error`, preferring the thrown value's own `.message` when it's already an
  `Error`/`DOMException`, so callers get a working `.stack` and `err instanceof Error`
  instead of an opaque string.
- **Unknown-field rejection** on `TransportOptions`/`MailInput`/`AuthInput` — a typo'd
  JS field key (e.g. `scecure`) is now a hard error instead of being silently ignored.

## Known, documented limitation

The `DATA` phase's dot-stuffing currently builds the entire stuffed message in memory
before writing it — for a large base64-encoded attachment that's 2–3x the message size
held in memory at once, worth keeping in mind given a Worker's default 128MB isolate
memory cap. Streaming it would require a chunked dot-stuffing API in the shared
`trustmailer-smtp-protocol` crate (used by the native SMTP transport too); it's a
documented follow-up, not something silently swept under the rug.

It shares one more limitation with the native SMTP transport: sending to multiple
recipients is all-or-nothing — the first `RCPT TO` a server rejects aborts the whole
transaction, rather than delivering to the recipients that were accepted and reporting
the rest as rejected the way nodemailer does. See the
[SMTP transport reference](/docs/transports/smtp/#the-smtp-envelope) for the full detail;
it applies identically here.

## Composing with DKIM

`SigningTransport` (from `trustmailer-dkim`) wraps this transport the same way it wraps
any other, at the Rust source level. See [DKIM signing](/docs/dkim/) for the full
picture, including a known `wasm32`-specific dependency gap in `trustmailer-dkim` itself
and the fact that the shipped `createTransport()` JS binding here doesn't yet expose a
`dkim` option the way `trustmailer-node`'s does.

See also: the [runtime matrix](/docs/runtime-matrix/) for how this transport compares
across targets, [Security](/docs/security/) for the full audit picture, and the
[Cloudflare Workers quickstart](/docs/quickstart/cloudflare-workers/) for the minimal
setup path.
