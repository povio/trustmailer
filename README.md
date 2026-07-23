<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-lockup-dark.svg">
    <img src="assets/logo-lockup-light.svg" alt="trustmailer" width="380">
  </picture>
</p>

<p align="center">
  <a href="https://trustmailer.nesgarbo.com/docs/"><img alt="Documentation" src="https://img.shields.io/badge/docs-trustmailer.nesgarbo.com-1e97ac"></a>
  <a href="./AUDIT.md"><img alt="Security audit" src="https://img.shields.io/badge/security-independently%20audited%20twice-d9522f"></a>
  <a href="./LICENSE-MIT"><img alt="License" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-f3ce6e"></a>
</p>

A [nodemailer](https://nodemailer.com)-compatible mail library, Rust-powered, for every
runtime: native Rust, Node.js/Bun/Deno (one native addon, no per-runtime rebuild),
Cloudflare Workers (real SMTP over genuine outbound TCP, not an HTTP relay), and the
browser/other WASM hosts. Same message model, same transport contract — one Rust core,
four ways to call it. Built by [nesgarbo](https://nesgarbo.com).

`trust` because it's Rust (and because you shouldn't have to trust a JS mail stack's
transitive dependency tree with your SMTP credentials).

**📖 Full documentation, in English and Spanish, lives at
[trustmailer.nesgarbo.com](https://trustmailer.nesgarbo.com/docs/)** — a per-transport
option reference (SMTP, Sendmail, SES, Stream, JSON, Cloudflare Workers, browser/WASM),
a `Message`/`Addresses`/`Attachments` guide, an [error reference](https://trustmailer.nesgarbo.com/docs/errors/),
and a [real, reproducible comparison against nodemailer](https://trustmailer.nesgarbo.com/docs/comparison/)
— throughput numbers, an output-parity test, and nodemailer's currently-published
advisories, all backed by scripts in [`benchmarks/`](./benchmarks). This README stays
intentionally shorter than the docs site; it's the "am I in the right place" overview,
not the reference.

## Why

Nodemailer is the de facto standard for composing and sending mail from JavaScript, but
it's Node-only — it reaches into `net`/`tls`/`child_process`, none of which exist in a
browser tab or a true sandboxed edge isolate (Cloudflare Workers). trustmailer keeps the
same mental model (a `Message`, a pluggable `Transport`, `createTransport`/`sendMail`)
but implements it once in Rust and exposes it everywhere:

- **Native Rust** — `trustmailer` crate, `cargo add trustmailer`.
- **Node.js, Bun, Deno** — `trustmailer-node`, a native addon via [napi-rs](https://napi.rs),
  API-shaped like `nodemailer.createTransport(...).sendMail(...)`. Node-API is a stable
  ABI, not a Node-specific one: the exact same compiled `.node` binary runs unmodified
  under Bun (native N-API support) and Deno (N-API support since Deno 2.0, including on
  Deno Deploy) — verified empirically for all three, see `crates/trustmailer-node/smoke-test.cjs`.
- **Cloudflare Workers** — `trustmailer-smtp-cfworkers`, real SMTP over the
  `cloudflare:sockets` `connect()` API (genuine outbound TCP, not an HTTP relay) —
  verified against a live `wrangler dev`/`workerd` run, see Security/Runtime sections below.
- **Browser / other WASM hosts** — `trustmailer-wasm`, via `wasm-bindgen`, for hosts that
  are a genuine sandbox with no native-code or raw-TCP escape hatch at all.

## Architecture

```
trustmailer-core           message model, address parsing, MIME builder, Transport trait
  ├─ trustmailer-smtp-protocol  pure SMTP protocol logic (commands/responses/dot-stuffing) [any runtime]
  │    ├─ trustmailer-smtp        SMTP over tokio TCP (TLS/STARTTLS, auth, pool, proxy)    [native]
  │    └─ trustmailer-smtp-cfworkers  SMTP over cloudflare:sockets connect()               [Cloudflare Workers]
  ├─ trustmailer-sendmail   pipes to a local `sendmail` binary                          [native]
  ├─ trustmailer-ses        Amazon SES v2 HTTPS API, hand-rolled SigV4                  [any runtime]
  ├─ trustmailer-dkim       DKIM signing (RSA-SHA256, relaxed/relaxed)                  [any runtime]
  ├─ trustmailer-stream     renders raw MIME instead of sending (tests, custom relays)  [any runtime]
  └─ trustmailer-json       renders a JSON message instead of sending (tests)           [any runtime]

trustmailer              facade crate: re-exports core + all native transports, feature-gated
trustmailer-node         napi-rs binding — createTransport()/sendMail() for Node.js/Bun/Deno
trustmailer-wasm         wasm-bindgen binding — createTransport()/sendMail() for browser/other WASM hosts
```

`trustmailer-core` and `trustmailer-smtp-protocol` have no I/O and compile to `wasm32` as-is:
address parsing, MIME building, and SMTP command/response handling are pure functions over
bytes/strings, shared by every binding — only the byte-stream I/O (`tokio` TCP vs. `cloudflare:sockets`
vs. nothing at all) differs per transport.

## Runtime support matrix

| Transport         | Native Rust | Node.js / Bun / Deno (napi addon) | Cloudflare Workers | Browser / other WASM |
|--------------------|:-----------:|:---------------------------------:|:-------------------:|:---------------------:|
| SMTP (TLS/STARTTLS) | ✅ | ✅ | ✅ — real TCP via `cloudflare:sockets`, not a relay | ❌¹ — no raw TCP sockets in a normal web page |
| Sendmail            | ✅ | ✅ | ❌² — no process spawning in the Worker isolate | ❌ — no process spawning, full stop |
| SES (HTTPS API)     | ✅ | ✅ | ✅ | ✅ — `reqwest` uses `fetch` under `wasm32` |
| Stream / JSON       | ✅ | ✅ | ✅ | ✅ |
| DKIM signing        | ✅ (`SigningTransport` wraps any transport) | ✅ (`dkim` option on `createTransport`) | ✅ (compose with `trustmailer-dkim` directly) | ✅ |

¹ Checked, not assumed: Chrome does have a raw-TCP [Direct Sockets API](https://developer.chrome.com/docs/iwa/direct-sockets),
but it's scoped to **Isolated Web Apps** — a separately packaged, signed, installed app
model, not a page loaded at a URL in a normal tab — so it doesn't apply to "browser" in
the sense this crate targets (a library a web page imports). No other browser ships
anything equivalent.

² Checked, not assumed: the Worker isolate genuinely cannot spawn a process — Cloudflare's
own docs are explicit that "Workers run on a V8 isolate with no disk and no way to spawn a
native process." [Cloudflare Containers](https://developers.cloudflare.com/containers/)
(a separate product — real Linux containers, with `exec()` support as of June 2026) *can*
run an actual `sendmail` binary, but that's a different compute primitive you'd reach for
directly, not something `trustmailer-smtp-cfworkers`'s Worker-isolate code can touch —
inside a container, you're just running native Rust (or nodemailer) like any other Linux host.

**Cloudflare Workers get real SMTP, not a relay — go verify it, don't take the README's word for it.**
Workers ship a genuine outbound-TCP API, `cloudflare:sockets`' `connect()`, with the platform
handling TLS for you (`secureTransport: "on"` for implicit TLS, `"starttls"` + `.startTls()`
for STARTTLS). The only restriction is port 25 (MX-to-MX relay, blocked — same as every major
cloud provider, to fight spam); ports 587/465 (SMTP *submission*, what `SmtpConfig` defaults to
and what real-world nodemailer-style usage actually means) are unrestricted. This is genuinely
verified, not assumed from documentation: `crates/trustmailer-smtp-cfworkers/test-worker/`
is a real `wrangler dev` project that sends a full MIME message over a real TCP SMTP
transaction to a local fake SMTP server, reproducible with:

```sh
cd crates/trustmailer-smtp-cfworkers
wasm-pack build --target web --out-dir test-worker/pkg
cd test-worker
node fake-smtp-server.cjs 2525 &
npx wrangler dev --local
curl "http://127.0.0.1:8787/?host=127.0.0.1&port=2525"
# {"ok":true,"info":{"accepted":["bob@example.com"],"response":"OK: queued as 12345", ...}}
```

A genuine, narrower limitation remains: an actual **browser tab** has no raw-TCP or
native-code API at all (no JS mail library can do SMTP from a web page either), and neither
does any other generic `wasm32` host that isn't specifically `workerd` (a plain Deno/Bun WASM
sandbox, `wasm-pack test`, etc. — `cloudflare:sockets` is a workerd built-in, not a web
standard). For those, either call the SES transport (a plain HTTPS call) or use the
stream/JSON transport to render the MIME message and hand it to your own relay/API over `fetch`.

## Status

Feature set is at parity with nodemailer's transport surface, including two security
fixes found by porting nodemailer's own regression tests (see below). Done:

- Address parsing ported from nodemailer's actual `addressparser` tokenizer/state
  machine: display names, quoted commas, RFC 5322 groups (with depth-limited nested-group
  flattening), comments-as-name, escapes, domain literals — infallible/permissive like
  the original, not a strict validator.
- MIME builder: multipart/alternative (text/watchHtml/amp/html), multipart/related
  (inline `cid:` images), multipart/mixed (attachments), calendar events (`icalEvent`,
  as a `text/calendar` alternative), `List-*` headers (with the RFC 2919 `List-ID`
  bare-domain form), RFC 2047 header encoding, RFC 2231 extended parameters
  (`filename*=UTF-8''...`) for non-ASCII/quote-containing attachment filenames,
  quoted-printable/base64.
- Every header value is CRLF-stripped before being written (see Security below).
- SMTP transport (native): implicit TLS + STARTTLS, PLAIN/LOGIN/XOAUTH2 auth, connection
  pooling, SOCKS5 proxy, OAuth2 access-token refresh (`OAuth2TokenProvider`), and a
  `SmtpConfig::from_url("smtps://user:pass@host:port")` connection-string constructor.
- SMTP transport (Cloudflare Workers): the same protocol logic (`trustmailer-smtp-protocol`)
  driven over `cloudflare:sockets` instead of `tokio` — real outbound TCP, platform-handled
  TLS, live-verified against `workerd` (see above).
- Sendmail, SES (SigV4 hand-rolled, no AWS SDK, with an endpoint override for
  VPC/testing), stream, JSON transports.
- DKIM signing (RSA-SHA256, relaxed/relaxed canonicalization), composable as
  `SigningTransport::new(signer, inner_transport)` over any other transport, and wired
  into `trustmailer-node`'s `createTransport({ dkim: {...} })`.
- A `well_known` services table generated from nodemailer's own `services.json`
  (86 providers/aliases), resolvable by name, alias, bare domain, or full address
  (`lookup_service("user@gmail.com")` works).

### Security

Two vulnerability classes were found (and fixed) by porting nodemailer's own security
regression tests rather than just its happy-path tests — both are the kind of bug that
only shows up when you deliberately go looking for the attacker's-eye view:

- **MIME header injection** (ported from `test/nodemailer/list-headers-test.js`,
  regression test for GHSA-268h-hp4c-crq3): any header value containing a CRLF could
  terminate that header early and inject arbitrary follow-on headers into the message.
  Fixed with universal sanitization at the single point every header is written
  (`mime/node.rs::write_header_line`), not per call site.
- **SMTP command injection** (ported from `test/smtp-connection/smtp-connection-test.js`):
  the `EHLO` name and `MAIL FROM`/`RCPT TO` envelope addresses are written directly into
  SMTP protocol commands. A CRLF in the EHLO name is now stripped at assignment
  (`SmtpConfig::with_ehlo_name`); a CRLF/`<`/`>` in an envelope address is now rejected
  before any bytes reach the socket (`SmtpConnection::send_transaction`).

Note: the address parser normalizes bare `\n` *and* bare `\r` to a space during
tokenization (`address.rs::tokenize`), so display names round-trip consistently
regardless of which line-ending style shows up in free text. This is purely a
display-name normalization concern, not a security boundary — the two SMTP-layer
fixes above are what actually close the injection path today, and are the correct
place for that check regardless (nodemailer validates in the same place, not in its
address parser).

A third bug, this one not from porting a test but from actually running the Cloudflare
Workers path live end-to-end (see above): `sendMail()`'s JS return value came back as an
empty `{}` in both `trustmailer-wasm` and `trustmailer-smtp-cfworkers`. `serde-wasm-bindgen`
serializes struct fields as a JS `Map` by default, and a `Map` has no own enumerable
properties, so `Response.json()`/`JSON.stringify()` silently rendered it as `{}` — no error,
just quietly-wrong data reaching the caller. Fixed by serializing with
`serialize_maps_as_objects(true)` in both crates. This is exactly the kind of bug that a
docs-only review or a compile-only check would never surface — it only showed up because
the actual JSON response was inspected end-to-end against a real `workerd` run.

The fixes above went through a **second, independent audit pass** afterward — specifically
tasked with re-verifying every fix and looking for what the first pass missed. It found
three more real issues (a reachable panic in the address parser, a bypassable OAuth2
credential-leak guard, a STARTTLS regression the first pass's own fix introduced), all now
fixed and regression-tested. The full, unabridged log of every finding across both
passes — file:line references, what was fixed, what was deliberately left open and
why — is public in [`AUDIT.md`](./AUDIT.md). See also
[trustmailer.nesgarbo.com/docs/security](https://trustmailer.nesgarbo.com/docs/security/)
for the narrative version.

### Test coverage

The test suite is substantially ported from **nodemailer's own `test/` directory**
(`../nodemailer` alongside this repo) — same fixtures, same attack payloads, and where
the architectures line up closely enough, the same expected values:

- **Address parser**: ~30 cases ported directly from `test/addressparser/`, including
  the group-nesting DoS-protection cases (depth 3000/10000 without a stack overflow).
- **DKIM**: two golden cross-implementation tests — the exact same RSA key and message
  signed independently by real nodemailer (via `node`, using its actual `lib/dkim`) and
  by trustmailer, asserting byte-identical `bh=`/`b=` values. Plus the relaxed-body
  hash test against nodemailer's real `test/dkim/fixtures/message1.eml` fixture.
- **MIME builder**: behavioral ports of `test/mail-composer/`, `test/mime-node/` and
  `test/nodemailer/list-headers-test.js` (including the CRLF-injection regression suite
  for all 7 `list.*` keys) — structure/ordering assertions rather than byte-exact output,
  since trustmailer's exact serialization (boundary format, default transfer encodings)
  differs from nodemailer's by design.
- **SMTP**: integration tests against a small hand-rolled plaintext SMTP server
  (`crates/trustmailer-smtp/tests/fake_smtp.rs`) exercising the full EHLO/AUTH/MAIL/
  RCPT/DATA transaction, auth success/failure, envelope/DATA rejection, injection
  rejection, and pool connection reuse — standing in for nodemailer's
  `smtp-server`-backed suite. Plus `SmtpConfig::from_url` parity tests ported from
  `test/shared/url-test.js`'s `parseConnectionUrl` cases.
- **SES / OAuth2**: against small hand-rolled HTTP/1.1 mock servers (no mocking
  framework, no real AWS/Google calls).
- **Sendmail**: spawns real `/bin/cat`/`/bin/sh` processes as sendmail stand-ins
  (Unix-only), rather than mocking `child_process.spawn` like nodemailer does.

- **Cloudflare Workers SMTP**: not something `cargo test` can drive (it needs the real
  `workerd` runtime, not just the `wasm32` target) — verified instead with a real
  `wrangler dev` project against a local fake SMTP server; reproduction steps are in the
  Runtime support matrix section above. `trustmailer-smtp-cfworkers`'s own `cargo test`
  covers what *is* portable to a native host: pure config/builder logic, with the
  `cloudflare:sockets`-dependent code `cfg`-gated to `wasm32` so it doesn't need
  `--target` to build and test cleanly.

Run everything with `cargo test --workspace --exclude trustmailer-node --all-features`
(174 tests across 23 suites as of this writing).

#### What wasn't ported, and why

nodemailer's suite is ~12,000 lines across ~40 files; the following were deliberately
left out rather than silently skipped:

- **`test/smtp-connection/smtp-connection-test.js`'s socket-buffering edge cases**
  (stray empty lines before/after the greeting, UTF-8 split across socket chunks,
  duplicate-listener/teardown races) test nodemailer's specific chunk-reassembly
  implementation, not SMTP protocol behavior — there's no trustmailer equivalent to
  port them against.
- **`test/smtp-connection/http-proxy-client-test.js`, `secure-socket-test.js`,
  `requiretls-test.js`, `starttls-buffer-test.js`**: HTTP CONNECT proxies (only SOCKS5
  is implemented), RFC 8689 REQUIRETLS, and low-level STARTTLS buffer-boundary handling
  aren't implemented yet.
- **`test/ethereal-test.js`, `test/nodemailer/ethereal-tls-test.js`,
  `test/nodemailer/get-test-message-url-test.js`**: wrap the Ethereal.email test-account
  service, which trustmailer doesn't integrate with.
- **`test/fetch/*`, `test/shared/shared-test.js`'s resolver/logger sections**: test
  nodemailer's internal `fetch`/logger polyfills, which trustmailer doesn't have
  (it uses `reqwest` directly).
- **`test/errors/errors-test.js`**: asserts nodemailer's string error-code constants
  (`err.code === 'ECONNECTION'`); trustmailer uses a typed `Error` enum instead, so the
  same test doesn't apply, though the enum covers comparable distinctions.
- **`test/syntax-compat.js`**: checks nodemailer's JS ships as Node 6-compatible syntax
  — not applicable to a Rust project.
- **`test/xoauth2/*`, `test/smtp-transport/oauth2-*`**: nodemailer's specific XOAUTH2
  provisioning/listener-leak edge cases; trustmailer's `OAuth2TokenProvider` has its own
  mock-server-backed test set instead, covering the same refresh-flow behavior.
- **Byte-exact MIME output**: `test/mail-composer/` and `test/mime-node/` assert
  nodemailer's *exact* serialized bytes (specific boundary strings, default 7bit/
  quoted-printable choices, header ordering). trustmailer's builder makes different
  (still RFC-compliant) choices by design, so these are ported as behavioral/structural
  assertions instead of string-equality checks.

## Quickstart

### Native Rust

```rust
use trustmailer::{Message, Transport};
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let config = SmtpConfig::new("smtp.example.com", 587)
        .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() });
    let transport = SmtpTransport::new(config);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .html("<b>Hi Bob!</b>")
        .build();

    let info = transport.send(&message).await?;
    println!("sent: {}", info.message_id);
    Ok(())
}
```

### Node.js / Bun / Deno

Identical code on all three — it's the same native addon underneath. (Deno needs a
`.cjs`/`.mjs` extension or a `package.json` with `"type"` set, per its CJS-detection rules.)

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

### Cloudflare Workers (real SMTP)

Build with `wasm-pack build --target web`, import the generated `.wasm` file as a module
(Wrangler's bundler turns it into a `WebAssembly.Module`), and `initSync` — not the
default async `init()` export, which needs a top-level `await` — see
`crates/trustmailer-smtp-cfworkers/test-worker/src/index.js` for the full working example.

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

### Browser / other WASM hosts

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

## Building

```sh
# native workspace (core + all native transports + facade)
cargo build --workspace --exclude trustmailer-node

# Node.js/Bun/Deno native addon (run from crates/trustmailer-node) — one build, all three runtimes
npx @napi-rs/cli build --platform --release
node smoke-test.cjs   # or: bun smoke-test.cjs / deno run -A smoke-test.cjs

# Cloudflare Workers SMTP (run from crates/trustmailer-smtp-cfworkers)
wasm-pack build --target web
# then see test-worker/ for a full wrangler dev reproduction against a local fake SMTP server

# Browser / other WASM hosts (run from crates/trustmailer-wasm)
wasm-pack build --target web
```

## License

Dual-licensed under either of [MIT](./LICENSE-MIT) or [Apache License, Version 2.0](./LICENSE-APACHE),
at your option.
