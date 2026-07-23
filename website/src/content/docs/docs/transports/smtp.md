---
title: SMTP transport
description: Full reference for trustmailer's native SMTP transport — TLS/STARTTLS, authentication, OAuth2, pooling, proxying, and well-known services.
---

The SMTP transport (`trustmailer-smtp`) is trustmailer's primary transport: a from-scratch
SMTP client over `tokio`, implementing the connect → `EHLO` → optional STARTTLS → optional
`AUTH` → `MAIL FROM`/`RCPT TO`/`DATA` transaction itself, rather than shelling out to a
system MTA or another crate. It's native-only (raw TCP + `tokio-rustls`), so it's available
from native Rust and from the Node.js/Bun/Deno native addon; browser/Workers hosts use
[`trustmailer-smtp-cfworkers`](/docs/quickstart/cloudflare-workers/) or `trustmailer-wasm`
instead — see the [runtime matrix](/docs/runtime-matrix/) for the full picture.

Everything below is verified against the actual `SmtpConfig` struct and its `with_*`
builder methods in `crates/trustmailer-smtp/src/config.rs` — if an option isn't listed
here, trustmailer doesn't have it (yet).

## Creating a transport

```rust
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};
use trustmailer::Transport;

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() });
let transport = SmtpTransport::new(config);
```

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  host: "smtp.example.com",
  port: 587,
  auth: { user: "user", pass: "pass" },
});
```

`SmtpConfig::new(host, port)` is the entry point in Rust; every other setting is a
`with_*` builder call chained onto it (the struct's fields are `pub`, but the builders
also normalize input — e.g. `with_ehlo_name` strips CRLF — so prefer them over field
assignment). The Node.js binding's `createTransport()` builds a `SmtpConfig` for you from
a plain options object; see [Node.js support](#nodejs-bun-deno-support) below for exactly
which fields it forwards.

## The SMTP envelope

`MAIL FROM`/`RCPT TO` — the addresses the SMTP protocol itself actually delivers to — are
computed from the message, not read off a separate config here. See
[Message → Envelope and `effective_envelope()`](/docs/message/#envelope-and-effective_envelope)
for how `to`/`cc`/`bcc`/`from` derive the default envelope, when to override it with
`.envelope(...)` (mailing-list bounce handling, `Bcc` recipients — deliverable but never
present in any header), and the `NoRecipients`/`NoSender` errors raised when neither the
headers nor an explicit override leave anything to send to.

:::caution[No partial-recipient delivery, unlike nodemailer]
With multiple recipients, nodemailer delivers to whichever `RCPT TO` addresses the server
accepts and reports the rest via `info.rejected`, even if some are refused — only total
rejection fails the send. `SmtpTransport::send` doesn't do this today: `send_transaction`
(`crates/trustmailer-smtp/src/connection.rs`) aborts the *entire* transaction on the
**first** non-`250` `RCPT TO` response — no `DATA` is sent at all, so a single bad address
in a large `to`/`cc` list blocks delivery to every other recipient too.
`SentInfo.rejected` is consequently always empty from this transport (`accepted` always
mirrors the full envelope) — there's no partial-failure information to report, because
partial failure can't happen; it's all-or-nothing. This applies to the Cloudflare Workers
SMTP transport too (`trustmailer-smtp-cfworkers`), which shares the same loop shape. If
your mailing-list use case needs per-recipient results, send one message per recipient
until this is addressed.
:::

## Connection basics: host, port, secure, require_tls

| Field | Type | Default | Description |
|---|---|---|---|
| `host` | `String` | — (required) | SMTP server hostname or IP. |
| `port` | `u16` | — (required) | SMTP server port. |
| `secure` | `bool` | `port == 465` | Connect with **implicit TLS**: the TLS handshake happens immediately after the TCP connect, before any SMTP command is sent. |
| `require_tls` | `bool` | `port == 587` | Demand a **STARTTLS** upgrade during the plaintext phase; the connection fails outright if the server doesn't advertise `STARTTLS`. |

`SmtpConfig::new` derives sensible defaults purely from the port number: port `465` sets
`secure: true` (implicit TLS, the traditional SMTPS convention), port `587` sets
`require_tls: true` (the modern submission-port convention), and any other port leaves
both `false`. Calling `.with_secure(true)` also flips `require_tls` to `false` (and vice
versa) since the two are mutually exclusive — `secure` encrypts from the first byte,
`require_tls` starts in plaintext and upgrades mid-conversation.

### Implicit TLS vs STARTTLS vs opportunistic upgrade

These are three different points in the connection lifecycle at which encryption can
begin, and trustmailer's connection code (`crates/trustmailer-smtp/src/connection.rs`)
handles all three explicitly:

1. **`secure: true`** — TLS wraps the socket before `EHLO` is ever sent. This is what
   port 465 has always meant.
2. **`require_tls: true`** — the connection starts in plaintext, sends `EHLO`, and if the
   server doesn't list `STARTTLS` in its capability response, the connect call fails
   immediately with `"server does not advertise STARTTLS"` rather than silently falling
   back to plaintext.
3. **Opportunistic upgrade (the default when neither of the above forces the outcome)** —
   even without `require_tls`, if the server's `EHLO` response advertises `STARTTLS`,
   trustmailer negotiates it anyway. The reasoning baked into the connection code: there's
   no good reason to leave a connection plaintext just because the caller didn't
   explicitly ask for TLS, when the server just offered it for free.

:::tip[Independently audited]
The opportunistic-upgrade behavior above is deliberate, not incidental — an earlier
iteration made the upgrade *unconditional* (no way to decline it), which broke internal
relays presenting a self-signed certificate that previously worked fine in plaintext. A
second, independent audit pass caught the regression and the `disable_starttls` escape
hatch below was added specifically to fix it. See [Security](/docs/security/) for the
full account of what was found and fixed.
:::

Use `with_disable_starttls(true)` to decline the opportunistic upgrade even when the
server offers `STARTTLS` — for an internal relay whose self-signed/untrusted certificate
would otherwise fail against trustmailer's hardcoded `webpki-roots` trust store (there's
no custom CA option yet, so this is currently the only way to keep talking to such a
relay). This flag is **ignored** when `require_tls` is set, since that setting explicitly
demands TLS and a hard failure is the safer outcome there.

```rust
// Internal relay, self-signed cert, on 587 but doesn't need STARTTLS forced:
let config = SmtpConfig::new("relay.internal", 587)
    .with_disable_starttls(true);
```

## Authentication

`Auth` (from `trustmailer-smtp-protocol`) has three variants, each mapping to a
standard SASL mechanism:

| Variant | SASL mechanism | Fields |
|---|---|---|
| `Auth::Plain` | `PLAIN` | `username`, `password` |
| `Auth::Login` | `LOGIN` | `username`, `password` |
| `Auth::XOAuth2` | `XOAUTH2` | `username`, `access_token` |

```rust
let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() });
```

Before sending `AUTH`, the connection checks that the server's `EHLO` response actually
advertised that specific mechanism (e.g. `AUTH PLAIN LOGIN`) and fails fast with
`"server does not advertise AUTH <mechanism>"` if not — rather than sending credentials
into the void and getting a generic rejection back. `Auth::Plain`/`Auth::XOAuth2` send
their credentials as a single `AUTH <mechanism> <base64>` command (an initial response);
`Auth::Login` does the RFC 4954 challenge/response dance (`AUTH LOGIN` → server prompts →
base64 username → server prompts → base64 password).

### The `allow_insecure_auth` guard

| Field | Type | Default | Description |
|---|---|---|---|
| `allow_insecure_auth` | `bool` | `false` | Permits `AUTH` to be sent over a connection that is neither `secure` nor upgraded via STARTTLS. |

:::caution[Security feature, not just a flag]
By default, trustmailer **refuses to send `AUTH` in cleartext**. If authentication is
configured but the connection ended up neither `secure: true` nor STARTTLS-upgraded (for
example, a misconfigured internal relay on a nonstandard port that doesn't offer
`STARTTLS`), `SmtpConnection::connect` returns an error —
`"refusing to send AUTH over a non-TLS connection (set SmtpConfig::allow_insecure_auth to
override)"` — instead of quietly putting a password on the wire in plaintext. This is a
realistic misconfiguration case, not a hypothetical: it's exactly the kind of mistake that
leaks credentials to anyone who can observe the connection. Set
`with_allow_insecure_auth(true)` only when you've deliberately decided that's acceptable
(e.g. a loopback connection to a local test server).
:::

```rust
// Local mail-catcher on plaintext port 1025 — no TLS available, and that's fine here.
let config = SmtpConfig::new("localhost", 1025)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() })
    .with_allow_insecure_auth(true);
```

### OAuth2 (XOAUTH2)

For providers like Gmail that support OAuth2 instead of a static password, `OAuth2Config` +
`OAuth2TokenProvider` (in `trustmailer-smtp::oauth2`, native Rust only — see the Node.js
section below) handle the token refresh flow: register an OAuth app, obtain a
`refresh_token` once via the provider's consent flow, then let the provider exchange it for
short-lived access tokens on demand.

```rust
use trustmailer::smtp::{OAuth2Config, OAuth2TokenProvider, SmtpConfig, SmtpTransport};

let oauth2 = OAuth2Config::new(
    "user@gmail.com",
    "client-id.apps.googleusercontent.com",
    "client-secret",
    "refresh-token",
);
let provider = OAuth2TokenProvider::new(oauth2);

let config = SmtpConfig::new("smtp.gmail.com", 465)
    .with_secure(true)
    .with_auth(provider.to_auth().await?);
let transport = SmtpTransport::new(config);
```

| Field | Type | Default | Description |
|---|---|---|---|
| `user` | `String` | — (required) | Mailbox to authenticate as. |
| `client_id` | `String` | — (required) | OAuth2 client ID. |
| `client_secret` | `String` | — (required) | OAuth2 client secret. |
| `refresh_token` | `String` | — (required) | Long-lived refresh token from the consent flow. |
| `access_url` | `String` | `https://accounts.google.com/o/oauth2/token` | Token endpoint; override via `.with_access_url(...)` for a non-Google provider. |

`OAuth2TokenProvider::get_access_token()` (used internally by `to_auth()`) caches the
fetched access token until shortly before its `expires_in` lapses (a 60-second safety
margin absorbs clock skew and in-flight latency), so repeated calls — or multiple pooled
connections sharing one provider — don't each re-hit the token endpoint.

:::caution[HTTPS-only, with a narrow exception]
`access_url` is rejected outright unless it's `https://`, **except** for an exact loopback
authority (`127.0.0.1`, `localhost`, or `[::1]`, optionally with a port/path) — that
exception exists purely so this crate's own tests can point at a local mock token
endpoint. The match is exact-authority, not `starts_with`: `http://127.0.0.1.evil.com` or
`http://localhost@evil.com` are correctly rejected, not treated as loopback. An earlier,
less careful version of this check used a bare `starts_with` and was caught and fixed
during a second, independent security audit — see [Security](/docs/security/) for details.
Sending `client_secret`/`refresh_token` to a plaintext, attacker-reachable URL would leak
both permanently.
:::

## Well-known services

`SmtpConfig::for_service(name)` looks up host/port/secure for a well-known provider by
name, alias, bare domain, or full email address — case- and punctuation-insensitive — via
`trustmailer_core::well_known::lookup_service`. Roughly 85 providers are recognized,
ported from nodemailer's own `well-known/services.json`; a few examples straight from the
table:

| Lookup key | Resolves to |
|---|---|
| `"gmail"` / `"Google Mail"` / `"gmail.com"` / `"user@gmail.com"` | `smtp.gmail.com:465`, `secure: true` |
| `"outlook"` / `"hotmail"` | `smtp-mail.outlook.com:587`, `secure: false` (STARTTLS) |
| `"ses"` (defaults to `us-east-1`; region-specific keys like `"sesEuWest1"` also exist) | `email-smtp.us-east-1.amazonaws.com:465`, `secure: true` |

```rust
let config = SmtpConfig::for_service("gmail")
    .expect("unknown service")
    .with_auth(Auth::Plain { username: "user@gmail.com".into(), password: "app-password".into() });
```

```js
const transporter = createTransport({
  service: "gmail",
  auth: { user: "user@gmail.com", pass: "app-password" },
});
```

`for_service` returns `None` for an unrecognized name — it never guesses at a host, so
check the result (or handle the `unwrap`/`expect` panic path deliberately) rather than
assuming every string resolves.

## Connection pooling

`SmtpTransport::pooled()` (or `SmtpPool::new(config)` directly) wraps a config in a pool
that reuses authenticated connections across sends instead of dialing, TLS-handshaking,
and authenticating fresh for every message — mirroring nodemailer's `pool: true` option.

```rust
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};
use trustmailer::Transport;

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() })
    .with_pool_size(10);
let pool = SmtpTransport::new(config).pooled();

for message in messages {
    pool.send(&message).await?;
}
pool.close().await;
```

| Field | Type | Default | Description |
|---|---|---|---|
| `pool_size` | `usize` | `5` | Max concurrent connections the pool will hold open (checkout blocks past this via a `tokio::sync::Semaphore` permit). |

On checkout, a pooled connection isn't handed back blindly: the pool sends a `NOOP` first
to confirm it's still alive, since SMTP servers commonly drop idle connections after a
timeout of their own. A dead connection is discarded and the pool either tries the next
idle connection or dials a fresh one — so a stale connection surfaces as a transparent
reconnect, not a raw I/O error on your first real command. On checkin (after a successful
send), the connection goes back into the idle list only if it's under `pool_size`;
otherwise it's dropped. On a failed send, the connection is torn down (`QUIT`) rather than
returned to the pool, since its state after an error is unknown. `pool.close()` sends
`QUIT` to every currently-idle connection; in-flight sends are unaffected.

:::note[Node.js/Bun/Deno]
Connection pooling is currently native-Rust-only — `TransportOptions` in
`trustmailer-node` doesn't expose a `pool`/`pool_size` field, so `createTransport()` always
returns a single-connection transport. See [Node.js support](#nodejs-bun-deno-support).
:::

## SOCKS5 proxying

`ProxyConfig` tunnels the SMTP connection through a SOCKS5 proxy (mirroring nodemailer's
`proxy: "socks5://..."` option). Only SOCKS5 is supported today — HTTP CONNECT proxies
aren't implemented.

```rust
use trustmailer::smtp::{Auth, ProxyConfig, SmtpConfig, SmtpTransport};

let proxy = ProxyConfig::new("proxy.internal", 1080)
    .with_auth("proxy-user", "proxy-pass"); // omit .with_auth(...) for an unauthenticated proxy

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() })
    .with_proxy(proxy);
let transport = SmtpTransport::new(config);
```

`ProxyConfig::parse("socks5://user:pass@proxy.internal:1080")` parses a proxy URL
directly (also accepting the `socks5h://` scheme as an alias) if you'd rather not build it
field by field.

:::caution[No silent downgrade to anonymous auth]
`ProxyConfig`'s `username`/`password` fields are public, so it's possible to construct one
with only *one* of the two set (bypassing `with_auth`). trustmailer treats that as a hard
configuration error at connect time rather than silently falling back to anonymous SOCKS5
authentication — a caller who set a username clearly intended authenticated proxying, and
silently downgrading that would be a security regression the caller never asked for.
:::

:::note[Node.js/Bun/Deno]
Like pooling, SOCKS5 proxying is currently native-Rust-only — not exposed through
`trustmailer-node`'s `TransportOptions`.
:::

## Timeouts

| Field | Type | Default | Description |
|---|---|---|---|
| `connect_timeout` | `Duration` | `30s` | Caps the initial TCP connect **and** the TLS/STARTTLS handshake. |
| `command_timeout` | `Duration` | `60s` | Caps any single SMTP command's write+read round trip, including reading a multi-line response in full. |

Every network operation in the connection — connect, TLS handshake, each command,
reading each (possibly multi-line) response, and the final `DATA` write — goes through
one shared timeout wrapper, so a server that stalls at any single step surfaces a clear
`"operation timed out after ..."` transport error instead of hanging the caller forever.
Response parsing is also bounded independently of the timeout: a single response line is
capped at 64 KiB, and a multi-line response is capped at 1000 lines / 256 KiB total, so a
misbehaving server can't grow client memory unbounded even within the timeout window.

```rust
use std::time::Duration;

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_connect_timeout(Duration::from_secs(10))
    .with_command_timeout(Duration::from_secs(20));
```

## The `from_url` connection-string shortcut

`SmtpConfig::from_url(url)` parses `smtp://[user[:pass]@]host[:port]` (plaintext/STARTTLS)
or `smtps://...` (implicit TLS) in one call — mirroring nodemailer's
`nodemailer.createTransport("smtps://user:pass@smtp.gmail.com")` shorthand.

```rust
let config = SmtpConfig::from_url("smtps://user%40gmail.com:app-password@smtp.gmail.com")?;
let transport = SmtpTransport::new(config);
```

Notes on the parsing behavior, all verified by the crate's own tests:

- The port defaults per scheme when omitted: `465` for `smtps://`, `587` for `smtp://`.
- Userinfo (`user:pass@`) is percent-decoded (`%40` → `@`, etc.), so an email-address
  username works directly in the URL.
- A password-only credential (`smtps://:api-key@host`) is supported — the username is
  simply empty — for providers that authenticate with a bare API key.
- Only the authority (`user:pass@host:port`) is used; any path, query string, or fragment
  on the URL is parsed and discarded, not rejected.
- An unsupported scheme (anything but `smtp`/`smtps`) or a missing host is a hard parse
  error, not a best-effort fallback.

`from_url` is currently native-Rust-only; it isn't wired into `trustmailer-node`'s
`TransportOptions` today (which takes `host`/`port`/`secure` as separate fields instead).

## Verifying the configuration

Every transport (SMTP included) implements `verify()`, mirroring nodemailer's
`transporter.verify()`: for SMTP this opens a real connection (going through the full
connect/TLS/STARTTLS/AUTH sequence above), sends a `NOOP`, then closes it — a way to
confirm host/port/credentials are correct before you're relying on it mid-send.

```rust
transport.verify().await?; // Ok(true) on success, Err(...) with the reason otherwise
```

```js
const ok = await transporter.verify();
```

## Full `SmtpConfig` reference

| Field | Type | Default | Set via |
|---|---|---|---|
| `host` | `String` | — (required) | `SmtpConfig::new(host, port)` |
| `port` | `u16` | — (required) | `SmtpConfig::new(host, port)` |
| `secure` | `bool` | `port == 465` | `.with_secure(bool)` (also flips `require_tls`) |
| `require_tls` | `bool` | `port == 587` | derived from `.with_secure(bool)`, or set directly |
| `auth` | `Option<Auth>` | `None` | `.with_auth(Auth::Plain \| Login \| XOAuth2 { .. })` |
| `ehlo_name` | `String` | `"localhost"` | `.with_ehlo_name(name)` (CRLF stripped) |
| `pool_size` | `usize` | `5` | `.with_pool_size(n)` |
| `proxy` | `Option<ProxyConfig>` | `None` | `.with_proxy(ProxyConfig)` |
| `connect_timeout` | `Duration` | `30s` | `.with_connect_timeout(Duration)` |
| `command_timeout` | `Duration` | `60s` | `.with_command_timeout(Duration)` |
| `allow_insecure_auth` | `bool` | `false` | `.with_allow_insecure_auth(bool)` |
| `disable_starttls` | `bool` | `false` | `.with_disable_starttls(bool)` |

`SmtpConfig` also implements `Debug` by hand rather than deriving it, specifically so
`auth`/`proxy` never print plaintext credentials if a config is ever logged.

## Node.js/Bun/Deno support

`trustmailer-node`'s `createTransport()` builds a `SmtpConfig` from a plain JS object, but
it currently forwards a narrower set of fields than native Rust supports directly:

| JS field | Maps to |
|---|---|
| `service` | `SmtpConfig::for_service(service)` |
| `host` | `SmtpConfig::new(host, ..)` |
| `port` | `SmtpConfig::new(.., port)` (defaults to `587` if omitted and `service` isn't set) |
| `secure` | `.with_secure(secure)` |
| `auth.user` / `auth.pass` | `.with_auth(Auth::Plain { username, password })` |
| `dkim.domainName` / `dkim.keySelector` / `dkim.privateKey` | wraps the transport in DKIM signing (not SMTP-specific) |
| `sendmail` / `path` | switches to the sendmail transport entirely, bypassing SMTP |
| `streamTransport` / `jsonTransport` | switches to a non-sending transport entirely |

**Not currently exposed to Node/Bun/Deno** (native Rust only, for now):
`require_tls`, `ehlo_name`, `pool`/`pool_size`, `proxy`, OAuth2 auth (only `Auth::Plain` is
reachable from JS today), `connect_timeout`/`command_timeout`, `allow_insecure_auth`,
`disable_starttls`, and `from_url`. If your Node.js workload needs any of those, the native
Rust crate is the way to get them today — see the
[Node.js quickstart](/docs/quickstart/nodejs/) and the
[native Rust quickstart](/docs/quickstart/native-rust/) for both entry points, and the
[runtime matrix](/docs/runtime-matrix/) for the complete per-runtime feature picture.

:::tip[Security guarantees still apply]
Even though the STARTTLS/`allow_insecure_auth` fields aren't independently *settable*
from JS yet, the underlying protections are: `createTransport()` always builds a
`SmtpConfig` with `disable_starttls: false` and `allow_insecure_auth: false` (the safe
defaults), so opportunistic STARTTLS and the plaintext-AUTH guard both apply exactly as
described above, regardless of which runtime is calling in.
:::
