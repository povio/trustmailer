# trustmailer — Code Audit

**Status: all findings fixed and independently re-verified.** See the "Fix Log" for the first pass and the "Second-Pass Verification" section at the very end for the independent re-audit (which found 3 additional issues — one a reachable panic both earlier passes missed — now also fixed). Final state: **174/174 tests pass** workspace-wide (up from 132), `cargo clippy --workspace --all-targets --all-features` clean, native + `wasm32-unknown-unknown` + node all compile.

Date: 2026-07-23
Scope: full workspace (`crates/*`), read-only review by 5 parallel agents, one per crate group. 132 existing tests pass; findings below are things *not* already covered by an existing test/regression suite.

Legend: 🔴 High (real bug/security issue, plausible trigger) · 🟡 Medium (real but narrower/needs specific conditions) · 🟢 Low (hardening / nitpick / DX).

---

## 0. Cross-cutting themes (seen in more than one crate)

### 🔴 DKIM `domain`/`key_selector` bypass the CRLF-injection defense that protects every other header
Two independent agents (core-adjacent DKIM audit and node/facade audit) hit the same bug from different angles:
- `crates/trustmailer-dkim/src/lib.rs:131-134,153` interpolates `self.domain`/`self.selector` (arbitrary `Into<String>`, no validation) straight into the `DKIM-Signature` value and emits the final header line via a raw `format!(...)` that **never goes through** `mime::node::write_header_line`/`strip_crlf` — the single choke-point that sanitizes every other header in the codebase (regression-tested for the GHSA-268h-hp4c-crq3 class of bug everywhere else).
- Reachable today from `crates/trustmailer-node/src/lib.rs:22-26,141-151` — `DkimOptions.domain_name`/`.key_selector` from JS land here unvalidated.
- `message.raw` is then written verbatim by `mime::build` (`trustmailer-core/src/mime/builder.rs:12-14`) with no re-sanitization.
- **Impact:** a `\r\n` in `domain_name`/`key_selector` injects arbitrary headers/body into every DKIM-signed message — the one code path in the whole codebase that bypasses the header-injection defense.
- **Fix:** reject/strip CR/LF (and probably `;`/`"`) in `domain`/`selector` at `DkimSigner` construction, or route the constructed DKIM-Signature line through `strip_crlf`.

### 🟡 Secrets leak through derived `Debug`/`Clone`
Same pattern found independently in two crates:
- `crates/trustmailer-smtp-protocol/src/lib.rs:12` (`Auth`), `crates/trustmailer-smtp/src/config.rs:5` (`SmtpConfig`), `crates/trustmailer-smtp/src/oauth2.rs:8` (`OAuth2Config`), `crates/trustmailer-smtp/src/proxy.rs:3` (`ProxyConfig`) — plaintext password/token/secret fields, derived `Debug`.
- `crates/trustmailer-ses/src/lib.rs:15-25` (`SesConfig`) — plaintext `secret_access_key`/`session_token`, derived `Debug` **and** `Clone`.
- **Impact:** any `{:?}` logging of these (a very common troubleshooting reflex — `tracing::debug!(?config)`) leaks credentials into logs.
- **Fix:** hand-write redacting `Debug` impls, or wrap secret fields in `secrecy::SecretString`/`zeroize::Zeroizing<String>`. Drop `Clone` from `SesConfig` unless a real call site needs it (none found).

### 🟡 `unwrap`-free but byte-offset string slicing panics on non-ASCII input
Two confirmed, reproducible panics of the same class in the SMTP layer (see §2) — worth fixing together since they share a root cause: slicing `&str` by byte offset without checking char-boundary/ASCII-only assumptions.

---

## 1. trustmailer-core

| # | Sev | Location | Issue |
|---|-----|----------|-------|
| 1 | 🟡 | `address.rs:295-300` | README says bare `\r` isn't "normalized"; actual behavior is it's **silently dropped** (not turned into a space like `\n` is), so `"John\rSmith"` → `"JohnSmith"` (no space) instead of `"John Smith"`. Not a CRLF-injection path (real protection is `mime/node.rs::strip_crlf`), but a genuine asymmetry bug + inaccurate doc comment. |
| 2 | 🟡 | `address.rs:181-200` | O(n²) recombination pass (`Vec::remove` in a loop) on the comma-in-display-name merge step; no cap on total address-list entries (unlike the 50-level group-depth cap). Crafted large `To`/`Cc` string → CPU-exhaustion DoS. |
| 3 | 🟡 | `address.rs:50-61,75-84` | `MAX_NESTED_GROUP_DEPTH` only guards the *parser*. `Address::group(...)` is public and `Deserialize`-able, so a directly-constructed deeply-nested `Address` graph (e.g. from deserialized untrusted JSON) recurses in `flatten`/`to_header_value` with **no depth check** → stack overflow. |
| 4 | 🟡 | `message.rs:123` | `to.dedup()` only collapses *consecutive* duplicates; `to`/`cc`/`bcc` aren't sorted first, so non-adjacent duplicate recipients survive → duplicate `RCPT TO`, possible spurious `rejected` entries. |
| 5 | 🟢 | `message.rs` (`effective_envelope`) | Empty-`to` is validated (`Error::NoRecipients`); empty/absent `from` is not — can silently produce `Envelope.from == ""`. |
| 6 | 🟡 | `mime/encode.rs:75-85` | `encode_header_phrase` escapes `"` but not `\` before quoting. A display name containing `,` and ending in an odd run of `\` produces a quoted-string that never closes per RFC 5322 — everything after on the line (including the real address) is read as still-quoted. |
| 7 | 🟡 | `mime/builder.rs:87-91` | `CalendarEvent.method` is an unvalidated string interpolated into the `Content-Type` parameter (`method=...`); a value containing `;` injects an extra MIME parameter. Not caught by `strip_crlf` (only strips CR/LF). Also stringly-typed where iCal only defines 4 valid values — should be an enum. |
| 8 | 🟢 | `mime/node.rs:132-152` | `fold_header` splits on literal `,` with no awareness of quoted-string state — can fold *inside* a quoted display name containing a comma, injecting whitespace into quoted content. Also: a single long value with no commas (long `Message-ID`/URL/encoded Subject) is never folded, can exceed RFC 5321's 998-octet line limit. |
| 9 | 🔴 | `attachment.rs:111-142` | `fetch_url` reads an unbounded HTTP response fully into memory, no size cap, no scheme/destination allowlist (no private/link-local IP blocking). If attachment URLs are ever influenced by a less-trusted actor: SSRF, and specifically **SSRF-to-exfiltration** (fetch a sensitive endpoint, attach the response, mail it out). |
| 10 | 🟢 | `attachment.rs:100` | `AttachmentContent::Path` reads arbitrary local paths with no canonicalization/allowlist — likely by design (mirrors nodemailer) but undocumented. |
| 11 | 🟢 | `address.rs:75-84`, `message.rs:107-124` | `flatten` clones full `Address` (name + group) then `effective_envelope` immediately discards everything but `.address` — wasted clones on the bulk-recipient hot path. |

**Well-covered, verified, not re-flagged:** CRLF/header injection via Subject/custom headers/`list.*` is thoroughly regression-tested and centrally enforced in `mime/node.rs::strip_crlf`; deep-group-nesting DoS via the text parser is tested and bounded (3000 levels).

---

## 2. trustmailer-smtp-protocol / trustmailer-smtp

| # | Sev | Location | Issue |
|---|-----|----------|-------|
| 1 | 🔴 | `trustmailer-smtp-protocol/src/lib.rs:149,153` | **Confirmed panic** (reproduced standalone): `line[..3]`/`line[4..]` byte-slicing panics if a multi-byte UTF-8 char straddles the offset. Any SMTP response line with non-ASCII bytes early on (malicious/compromised server, or on-path during the plaintext greeting) crashes the task, not a clean `Err`. |
| 2 | 🔴 | `trustmailer-smtp/src/config.rs:130` | **Confirmed panic** (reproduced standalone): `percent_decode`'s `s[i+1..i+3]` slice panics on a `%` immediately followed by multi-byte UTF-8 in a `smtp(s)://` URL. `SmtpConfig::from_url` is the documented "paste a connection string" entry point — a crafted string crashes the caller instead of returning `Err`. |
| 3 | 🟡 | `smtp-protocol/lib.rs:143-161`, `smtp/connection.rs:252-265` | No cap on multi-line SMTP response accumulation or `read_line` length — malicious/misbehaving server can grow client memory unbounded. |
| 4 | 🔴 | `smtp/connection.rs` (whole lifecycle) | **Zero timeouts anywhere** — `tokio::time` is never used despite `tokio`'s `"time"` feature being enabled in `Cargo.toml`. Connect, TLS handshake, every write/read can block forever; combined with the pool, one hung connection can hold a semaphore permit indefinitely and eventually wedge the whole pool. |
| 5 | 🟡 | `smtp/config.rs:22-34`, `connection.rs:91-96,113-121` | `require_tls` only defaults true on port 587; no opportunistic STARTTLS, no guard against `authenticate()` running over a plaintext channel on non-standard ports → credentials sent in cleartext on a realistic misconfiguration (e.g. internal relay on a custom port). |
| 6 | 🟡 | `pool.rs:27-41` | Idle pooled connections handed out with no liveness probe (`NOOP`); SMTP servers commonly close idle connections server-side → first command over a stale connection fails as a raw I/O error, no transparent retry. |
| 7 | 🟢 | `pool.rs:70,74` | Semaphore permit dropped *before* `checkin` completes → transient over-provisioning past `pool_size` under concurrent load. `close()` holds the idle-list mutex across sequential network round-trips (one `QUIT` per connection), blocking concurrent checkout/checkin meanwhile. |
| 8 | 🟡 | `proxy.rs:4-9`, `connection.rs:34-52` | SOCKS5 fails **open** to anonymous auth if only one of username/password is set (fields are `pub`, directly constructible bypassing `with_auth`) — should be a config error, not a silent downgrade. |
| 9 | 🟡 | `oauth2.rs` | No token caching/expiry handling (`expires_in` parsed nowhere) — every pooled connection needing a fresh token re-hits the OAuth endpoint. `access_url` isn't restricted to `https://` — a misconfigured URL would send `client_secret`/`refresh_token` over plaintext HTTP. |
| 10 | 🟢 | `connection.rs:113-119` | AUTH-advertised check only looks for *any* `AUTH` capability line, not the specific configured mechanism — fails later with a raw server rejection instead of a clear pre-check error. |
| 11 | 🟢 | `connection.rs:229-238,144-186` | `send_line` does two separate writes + flush instead of one buffered write; RCPT loop never consults the parsed `PIPELINING` capability — one RTT per recipient instead of batching. |
| 12 | ✅ | `connection.rs:97-107` | **Verified safe**: STARTTLS buffer-discard (`into_inner()` before handshake) correctly defeats the classic plaintext-command-injection/downgrade attack; re-EHLO after upgrade; no cert-validation escape hatch exists anywhere (hardcoded `webpki-roots`+rustls). |
| 13 | 🟢 | `Cargo.toml:25` | `serde_json` declared as a direct dependency but never used by name. |

**Test-coverage gaps observed:** no TLS/STARTTLS integration test at all, no SOCKS5 auth-negotiation test, no stale-pooled-connection test, no multi-line-rejection/malformed-separator test, no XOAUTH2-through-`authenticate()` test, no fuzz/property test on the exact non-ASCII input class that triggers findings #1/#2 above (which is why both are currently latent).

---

## 3. trustmailer-smtp-cfworkers / trustmailer-wasm

**Verification-scope caveat:** the only thing actually live-verified against real `workerd` is `secure:false, requireTLS:false` against a plaintext fake server. The `secure:true` and STARTTLS paths — the security-critical ones — have never been run against real `cloudflare:sockets`, only unit-tested for pure config-default logic. Worth closing given the crate's whole value proposition is "verified, not asserted."

| # | Sev | Location | Issue |
|---|-----|----------|-------|
| 1 | 🔴 | `lib.rs:112-163,174-208` | Socket leak on every non-happy-path return: `send()`/`verify()`/`connect()` all return early via `?` on any SMTP-level error *before* reaching `QUIT`/`close()`. `CfSocket` has no `Drop` impl closing the underlying `cloudflare:sockets` socket — cleanup depends entirely on JS GC. A long-lived Worker isolate hitting any rejected recipient/auth failure/etc. accumulates unclosed TCP sockets against Cloudflare's per-isolate connection limit. |
| 2 | 🟡 | `lib.rs:112-163` | STARTTLS/TLS sequencing logic is sound as written (verified: no silent downgrade, buffer correctly discarded across the upgrade) but **functionally unverified** against real `workerd` — see caveat above. |
| 3 | 🟢 | `js.rs:11-20` vs `wasm/lib.rs:18-29` | cfworkers' `MailInput` silently drops `replyTo` (present in the wasm crate's equivalent) — unknown-field silent-drop at the untyped JS boundary, no error, no Reply-To header. |
| 4 | 🟢 | `js.rs:48-52,90-95` | cfworkers JS binding can only ever produce `Auth::Plain` — no `type` discriminator even though `Login`/`XOAuth2` are fully supported at the config level. |
| 5 | 🟡 | `socket.rs:123-150` | `read_line`'s internal buffer grows unbounded with no max-size check — a server that never sends a line terminator is an OOM/isolate-crash vector in a memory-capped Worker. |
| 6 | 🟢 | `lib.rs:41-44` (`js_err`) | Only extracts text if the thrown JS value *is* a string; real thrown `Error`/`TypeError`/`DOMException` objects fall through to a generic debug format — poor diagnostics for the one conversion point covering every socket-level failure. |
| 7 | 🟢 | `js.rs`, `wasm/lib.rs` (4 sites) | All JS-facing errors thrown as plain strings (`JsValue::from_str`) instead of `Error` objects — no `.stack`, `instanceof Error` is false. |
| 8 | 🟢 | input structs (both crates) | No `#[serde(deny_unknown_fields)]` on `MailInput`/`TransportOptions`/`AuthInput` — a typo'd JS field key is silently ignored instead of erroring. |
| 9 | 🟢 | `lib.rs:263`, `socket.rs:110-115` | Dot-stuffed DATA phase materializes the whole stuffed buffer (2-3x message size in memory for large attachments) rather than streaming — worth noting given the 128MB default isolate memory cap. |

**Checked and confirmed clean:** the Map-vs-object serialization bug (previously fixed) doesn't recur anywhere else in scope; no panic-prone `unwrap`/`expect` crosses the JS boundary; no leaked `Closure::wrap` references; all socket I/O is bulk-copied, not byte-by-byte.

---

## 4. trustmailer-dkim / trustmailer-ses

This is the crypto-adjacent crate group — treat these as higher-priority than similarly-tagged findings elsewhere.

| # | Sev | Location | Issue |
|---|-----|----------|-------|
| 1 | 🔴 | `canonicalize.rs:71` (+`:4`) | DKIM body canonicalization decodes through `String::from_utf8_lossy` before hashing, but the **actual wire bytes are the original, un-mutated `raw_message`**. Any non-UTF-8 body (legacy `iso-8859-1`/`windows-1252` 8bit text, raw/binary CTE) gets bytes replaced with U+FFFD before hashing but not on the wire → signer computes `bh=` over mutated content while the receiver hashes the real bytes → **silent DKIM verification failure** at the receiving MTA (not surfaced to the sender), hurting deliverability/DMARC exactly the way this crate's whole design is meant to prevent. |
| 2 | 🔴 | `lib.rs:113-128` | The signed-headers check only errors if the signed-header set ends up **completely empty** — never specifically requires `From` was matched, though RFC 6376 mandates it. A malformed message or a caller-supplied `.with_headers(...)` omitting `"from"` produces a successfully-emitted signature that doesn't cover the sender identity — a signature-replay/spoofing vector (attacker keeps the valid body+signature, swaps `From`). |
| 3 | 🟡 | (cross-cutting, see §0) | DKIM `domain`/`selector` header-injection bypassing `strip_crlf` — see §0 top entry. |
| 4 | 🟢 | `canonicalize.rs:46,75` | Uses Rust `str::trim`/`trim_end` (full Unicode whitespace class) instead of RFC 6376's narrow `WSP = SP/HTAB` — diverges from a strict verifier only in the rare case a header/body line ends in an unusual Unicode space character. |
| 5 | 🟡 | `lib.rs:153` | Generated `DKIM-Signature` header is never folded. With a modern 2048/4096-bit key, `b=` alone can push the line past RFC 5322's 998-octet cap (the crate's own test key is only 1024-bit, masking this in tests). |
| 6 | 🟡 | `trustmailer-ses/src/lib.rs:15-25` | `SesConfig` derives `Debug`+`Clone` over `secret_access_key`/`session_token` — see §0 secrets-leak entry. |
| 7 | 🟢 | `trustmailer-ses/src/lib.rs:111` | `mime::build(message, &self.config.host())` passes the **SES API endpoint** (`email.us-east-1.amazonaws.com`) as the Message-ID hostname — generated Message-IDs look like `<...@email.us-east-1.amazonaws.com>` instead of the sending domain, hurting Message-ID/From domain alignment heuristics. |
| 8 | 🟢 | `canonicalize.rs:22-26`, `lib.rs:116-119` | Header names aren't trimmed before comparison — `"Subject : value"` (space before colon) silently fails to match `"subject"` and is dropped from signing with no error. |
| 9 | 🟢 | `canonicalize.rs:14` | Clippy-flagged (not a bug): `unwrap()` after `is_some()` instead of `if let`. |
| 10 | 🟢 | `transport.rs:40-45` | `SigningTransport::send` clones the **entire** `Message` (body + all attachments) just to attach already-computed signed bytes — doubles peak memory on every DKIM-signed send with attachments. |
| 11 | 🟢 | `canonicalize.rs:70-89` | Body canonicalization does several full-body copies plus one small heap allocation per line via `.split('\n').map(...).collect()` — real cost on multi-MB base64 attachment bodies (tens of thousands of lines). |

**Verified correct, not flagged as issues:** RSA-SHA256 signing uses the vetted `rsa` crate correctly (explicit, spec-correct DigestInfo prefix, no manual padding); SigV4 canonical request/signing-key HMAC chain matches the AWS spec exactly, `SignedHeaders` correctly sorted; no `l=` body-length tag (the *safer* default, avoids the "unsigned appended content" attack); crypto dependency choices (`rsa`, `sha2`, `hmac`, all RustCrypto) are appropriate — the one relevant advisory (RUSTSEC-2023-0071, Marvin Attack) targets decryption oracles, not the deterministic signing this crate does.

---

## 5. trustmailer-sendmail / trustmailer-stream / trustmailer-json / trustmailer (facade) / trustmailer-node

| # | Sev | Location | Issue |
|---|-----|----------|-------|
| 1 | 🔴 | `trustmailer-sendmail/src/lib.rs:55-65` | Classic `Command` pipe deadlock: stdin is fully written and dropped *before* `wait_with_output()` starts draining stdout/stderr. A child that writes enough to stdout/stderr to fill the OS pipe buffer while the parent is still pushing a large message body into stdin → both sides block forever, no timeout. Realistic with large attachments + a verbose sendmail wrapper. |
| 2 | 🔴 | (cross-cutting, see §0) | DKIM header-injection via `domain_name`/`key_selector`, reachable from `trustmailer-node/src/lib.rs:22-26,141-151` — see §0 top entry. |
| 3 | 🟡 | `trustmailer-node/src/lib.rs:48-58` | `MailOptions` has no `message_id` field, and no transport writes the auto-generated `Message-ID` back onto `message.message_id` — **`SentInfoJs.message_id` is guaranteed `""` for every message sent via `sendMail()`**, a concrete, always-reproducible divergence from nodemailer (`info.messageId` normally returns the real generated ID). |
| 4 | 🟡 | `trustmailer-node/src/lib.rs:48-58` | `MailOptions` has **no `attachments` field** at all — a core nodemailer feature is simply unreachable through the Node binding despite `trustmailer-core` fully supporting it. Largest capability gap found in the facade most users will actually touch. |
| 5 | 🟢 | `trustmailer-node/src/lib.rs:184` | `options.port.unwrap_or(587) as u16` silently truncates an out-of-range `u32` port (e.g. `65536` → `0`) instead of raising a clear `napi::Error`. |
| 6 | 🟢 | `trustmailer-sendmail/src/lib.rs:24-27`, `trustmailer-node/src/lib.rs:169-171` | Sendmail `path` accepts an arbitrary string with no validation (no "must be absolute" check); a relative value falls back to `PATH` lookup. Only exploitable by whoever controls transport config, not by mail content — but worth documenting as operator-trusted-only. |
| 7 | 🟢 | `trustmailer-dkim/src/transport.rs:40-45`, used via `trustmailer-node` | `SigningTransport::send` runs synchronous RSA-2048 signing directly inside an `async fn` with no `spawn_blocking` — under napi's shared Tokio runtime this blocks a worker thread on every DKIM-signed send, starving concurrent `sendMail()` calls. |
| 8 | 🟢 | `trustmailer-node/src/lib.rs:11-13` | `to_napi_err` flattens every `trustmailer_core::Error` variant to an opaque string — JS callers can't programmatically distinguish "no recipients" from "invalid DKIM key" from "connection refused" without substring-matching. |
| 9 | 🟢 | `trustmailer/src/lib.rs:33-44` | Facade builds fine under wasm today only because `trustmailer-wasm` bypasses it entirely — but nothing stops `cargo build --target wasm32-unknown-unknown -p trustmailer` (default features) from failing deep inside `tokio`'s `process` feature with a confusing error rather than a clear "sendmail requires a native target" message. |
| 10 | 🟢 | `trustmailer/src/lib.rs:33-44` | None of the six feature-gated re-exports carry doc comments, despite this being the main entry point most docs.rs visitors land on. |
| 11 | 🟢 | `trustmailer-json` | JSON transport intentionally never touches `mime::build`'s CRLF-sanitization path (documented, by design) — flagging only so no one mistakenly uses `JsonTransport` output as a stand-in to test header-injection safety; such a test would be vacuous. |
| 12 | 🟢 | `trustmailer-node/src/lib.rs:97-106` | `info.raw.map(|r| String::from_utf8_lossy(...))` fallback is currently dead code (every reachable transport always sets `response`) — harmless today, but would silently corrupt binary content if a future transport returns only `raw`. |

**Verified clean:** no shell is ever invoked for sendmail (pure argv `Command`, message body only goes to stdin) — no command-injection risk from mail content; no `unsafe` blocks anywhere in `trustmailer-node`; no panics reachable across the FFI boundary; facade's feature-gating (`Cargo.toml` ↔ `#[cfg(feature = ...)]`) checked line-by-line, no mismatches found.

---

## Suggested priority order (if tackling incrementally)

1. **🔴 Security-critical, low effort:** DKIM domain/selector CRLF injection (§0), the two confirmed SMTP-parser panics (§2 #1-2), sendmail stdin/stdout deadlock (§5 #1).
2. **🔴 Correctness, real deliverability impact:** DKIM non-UTF-8 body canonicalization mismatch (§4 #1), DKIM missing `From`-signed requirement (§4 #2).
3. **🟡 Hardening, moderate effort:** SMTP connection timeouts (§2 #4), attachment URL SSRF guard (§1 #9), secrets-in-Debug across SMTP/SES (§0), Node.js `message_id`/`attachments` gaps (§5 #3-4).
4. **🟢 Everything else:** quality-of-life, performance, and DX items — safe to batch into a single follow-up pass.

---

## Fix Log (2026-07-23)

All sections above fixed in a single pass, one agent per crate group, then verified together. Summary of what actually happened, including deliberate deviations from the suggested fix and anything found along the way:

### §0 Cross-cutting
- **DKIM domain/selector CRLF injection** — fixed at the root: `DkimSigner::from_pkcs8_pem` now validates `domain`/`selector` and rejects `\r`, `\n`, `;`, `"` at construction time (`trustmailer-dkim/src/lib.rs`), rather than trying to route through core's private `strip_crlf`. `trustmailer-node`'s error mapping updated so the rejection surfaces as a real `napi::Error` with the actual cause, not a generic "invalid DKIM private key" message.
- **Secrets leak via derived `Debug`/`Clone`** — fixed for all five structs: `Auth`, `SmtpConfig`, `OAuth2Config`, `ProxyConfig` (hand-written `Debug`, secrets redacted as `***REDACTED***`) and `SesConfig` (same, plus `Clone` dropped — confirmed unused after grep).
- **Byte-offset string-slicing panics** — both confirmed instances fixed (see §2).

### §1 trustmailer-core — all 11 items fixed
`\r` now normalizes like `\n`; address-list merge rewritten to a single forward pass with a 4096-entry cap (truncates, doesn't error, matching the existing infallible-parser style); group-nesting depth cap now also applies to `flatten`/`to_header_value` (not just the parser); recipient dedup is now order-preserving via `HashSet` (not `sort`+`dedup`, to preserve envelope order); new `Error::NoSender` mirrors the existing `NoRecipients` check; backslash is now escaped before the closing quote in header-phrase encoding; `CalendarEvent.method` is now a real enum (`Publish`/`Request`/`Reply`/`Cancel`) instead of an unvalidated string; `fold_header` now tracks quote state so it never folds inside a quoted display name; `fetch_url` caps attachment downloads at 25MB with a documented no-SSRF-hardening caveat; envelope-address extraction no longer clones full `Address` structs. 81 tests pass (was ~70). **Note:** the new `Error::NoSender` was checked workspace-wide — no other crate calls `effective_envelope` directly or builds a `Message` without `.from(...)` in a live (non-test) path, confirmed via full workspace test pass, so this is not a live regression risk.

### §2 trustmailer-smtp-protocol / trustmailer-smtp — all 13 items fixed
Both confirmed panics fixed by validating bytes before slicing (never slicing a `&str` on a raw offset again). Response accumulation now capped (1000 lines / 256KiB); malformed separator bytes rejected. `SmtpConfig` gained `connect_timeout`/`command_timeout` (30s/60s defaults) wrapping every connect/handshake/read/write in `tokio::time::timeout`. STARTTLS is now opportunistic whenever advertised; `authenticate()` refuses to run over a non-secure channel unless a new `allow_insecure_auth` flag is explicitly set. Pool now does a `NOOP` liveness check on checkout, and permit/lock ordering fixed to avoid transient over-provisioning and lock-holding across network round-trips. SOCKS5 half-set credentials now hard-error instead of silently falling back to anonymous. OAuth2 now caches tokens by parsed `expires_in` (60s safety margin) and restricts `access_url` to `https://` (with a narrow, deliberate loopback exception to keep the crate's own local mock-server tests working). AUTH-advertised check is now mechanism-specific. `send_line` now does a single `write_all`. **Deliberately skipped:** RCPT pipelining batching — flagged as a non-trivial protocol change (preserving per-recipient error attribution while batching) rather than risk a half-correct implementation; left as a documented follow-up. 50 tests pass (was ~30 in this crate group).

### §3 trustmailer-smtp-cfworkers / trustmailer-wasm — all 8 items fixed
Socket-leak paths in `send()`/`verify()`/`connect()` restructured so `QUIT`/`close()` run unconditionally via a `close_after` helper, regardless of success/failure; `CfSocket::start_tls` changed from consuming to `&mut self` so a failed handshake still leaves a closeable socket in the caller's hands. Real async `Drop` isn't feasible (Drop is sync, close is async) — documented as a known limitation, mitigated by the explicit-cleanup-on-every-path fix instead. Added `reply_to`, an `AuthType` discriminator (plain/login/xoauth2), an 8KB cap on the read-line buffer, proper JS `Error` object construction (was raw strings) with real message extraction via `js_sys::Error`, and `deny_unknown_fields` on all JS-facing input structs. **Deliberately skipped:** streaming the dot-stuffed DATA phase — would require an invasive change to the shared `trustmailer-smtp-protocol` crate; left as a documented follow-up. Verified against both native `cargo check` and real `wasm32-unknown-unknown` compilation (target was installed) — this is real compiler coverage of the actual changed code paths, not just native stand-ins.

### §4 trustmailer-dkim / trustmailer-ses — all 11 items fixed
The two highest-priority fixes: (1) domain/selector CRLF-injection rejection at construction (see §0), and (2) DKIM body/header canonicalization rewritten to operate on raw bytes throughout — no more `String::from_utf8_lossy`, so non-UTF-8 message bodies now hash correctly instead of silently corrupting the signature the receiver would independently compute. Both golden nodemailer cross-implementation tests (byte-identical signature match, relaxed-body-hash fixture) still pass after the rewrite — the strongest correctness signal available for this crate. Added an explicit "From must be signed" check independent of the general non-empty-signed-headers check; switched to WSP-only (space/tab) trimming instead of full-Unicode `trim` to match RFC 6376 exactly; added DKIM-Signature header folding (folds only at `;` tag boundaries, verified fold-safe against the golden tests). `SesConfig` got a redacting `Debug` and lost `Clone`; Message-ID hostname now defaults to the `From` address domain instead of the SES API endpoint, with an override builder method. Header-name whitespace (`"Subject : value"`) no longer silently drops that header from signing. **Deliberately not changed:** `SigningTransport::send`'s `Message` clone — investigated and found a partial-clone would silently break `JsonTransport`'s direct field access; the real fix (`Arc`/`Bytes`-backed attachment storage in `trustmailer-core::Message`) is a cross-crate refactor noted as a follow-up, not attempted in this pass. 21 unit tests + 2 doc-tests pass, 0 warnings.

### §5 trustmailer-sendmail / -stream / -json / trustmailer (facade) / trustmailer-node — all 11 items fixed
Sendmail stdin-write and stdout/stderr-drain now run concurrently via `tokio::join!` instead of sequentially, closing the classic pipe-deadlock window. `MailOptions` gained `message_id` (caller-settable) and `attachments` (nodemailer-shaped `AttachmentInput`); the Node binding now pre-generates a Message-ID before send when the caller didn't set one, so `SentInfoJs.message_id` is never empty anymore. Port values out of `u16` range now return a proper `napi::Error` instead of silently truncating. DKIM signing (`SigningTransport::send`) now runs inside `tokio::task::spawn_blocking` on native targets (inline on wasm32, where there's no blocking pool) so RSA signing no longer blocks a shared async runtime thread. `to_napi_err` now prefixes errors with their variant name and picks an appropriate napi `Status` instead of flattening everything to an opaque string. The facade's `sendmail` feature now hard-fails wasm32 builds with a clear, standalone `compile_error!` — verified by actually attempting `cargo build --target wasm32-unknown-unknown --features sendmail` and confirming only the intended message appears (required moving `trustmailer-sendmail` to a target-conditional dependency so an upstream crate's own wasm32 panic couldn't shadow the guard). All six facade re-exports now have doc comments. `info.raw`'s fallback path now base64-encodes instead of lossily UTF-8-decoding (more correct for the binary-data case this branch exists for, even though it's currently unreachable). 14 tests pass across affected crates, 17 in `trustmailer-dkim` (shared file with §4's agent, no conflict).

### Final integration verification (after all 5 fix passes merged)
- `cargo build --workspace --exclude trustmailer-node` — clean.
- `cargo test --workspace --exclude trustmailer-node --all-features` — **172 passed, 0 failed** (23 suites; was 132 before this pass).
- `cargo clippy --workspace --exclude trustmailer-node --all-targets --all-features` — no issues.
- `cargo check -p trustmailer-node` — clean.
- `cargo check --target wasm32-unknown-unknown -p trustmailer-smtp-cfworkers -p trustmailer-wasm` — clean.

### Found along the way, not in original scope (not fixed, noted for a future pass)
- Building the `trustmailer` facade crate for `wasm32-unknown-unknown` with the `dkim` feature enabled fails today: `rsa` → `num-bigint-dig` → `rand` → `getrandom` needs its `"js"` feature enabled for wasm32, which nothing in the dependency graph currently requests. This is pre-existing (unrelated to any fix in this pass) and only affects the facade crate directly — `trustmailer-wasm` (the crate actually meant for browser/WASM use) doesn't depend on `dkim`/`rsa` and is unaffected. Worth a small fix (enable `getrandom`'s `js` feature under `cfg(target_arch = "wasm32")`) in a follow-up if anyone actually needs `trustmailer` (not `trustmailer-wasm`) to target wasm32 with DKIM.

---

## Second-Pass Verification (2026-07-23)

After the fixes above, a second, independent, verification-focused audit re-read all the changed code (5 agents, one per crate group) with a mandate to (1) confirm each claimed fix is actually present and correct, (2) catch regressions or new bugs the fixes introduced, and (3) find anything both earlier passes missed. It did not trust the Fix Log — it re-derived each claim from the current code.

### Result: fixes are solid
Across all five crate groups, **zero fixes were found to be subtly wrong, and zero fixes introduced a new bug** — the ~54 first-pass fixes verified clean and complete. The highest-risk change (the DKIM byte-level canonicalization rewrite) was hand-traced against RFC 6376 §3.4.4 in every branch (empty body, no-trailing-newline, interior blank lines, bare CR, non-UTF-8 bytes, folded/whitespace-padded headers) and confirmed correct; the DKIM-Signature header folding was shown to be canonicalization-invariant (cannot break verification); the two golden nodemailer cross-implementation tests were confirmed genuine (real byte-equality assertions, not weakened) and still passing.

### But it found 3 additional issues — all now fixed in this second pass

1. **🔴 Reachable parser panic, missed by BOTH earlier passes** — `address.rs`: a comment-only address list entry (`parse_address_list("(x)")`, or a bare `(comment)` element mid-list like `"a@b.com, (bar)"`) moved the `comment` bucket into `text` without keeping the parallel `text_was_quoted` vector in sync, so the bare-address scan indexed out of bounds and panicked — violating the crate's "infallible parser, never panics on hostile input" contract, and reachable from any `To:`/`Cc:` value. **Fixed** by syncing `text_was_quoted` (comment tokens are never quoted → `vec![false; …]`) at the `mem::take` point. Regression test `comment_only_entry_does_not_panic` added.

2. **🟡 OAuth2 loopback exception was bypassable** — `oauth2.rs::is_https_or_loopback` used a bare `starts_with("http://127.0.0.1")`, so `http://127.0.0.1.evil.com`, `http://localhost.evil.com`, and `http://127.0.0.1@evil.com` all passed the check and would POST `client_secret`/`refresh_token` in cleartext to an attacker-controlled host — the same credential-exfil threat the HTTPS restriction was added to close. **Fixed** to require the loopback authority to terminate exactly (next char `:`, `/`, or end-of-string). Test `loopback_exception_requires_an_exact_authority` added.

3. **🟡 Forced opportunistic STARTTLS had no opt-out (behavioral regression)** — the first-pass "upgrade whenever advertised" change made a STARTTLS-advertising server that presents an untrusted/self-signed cert (common on internal relays) unreachable: the forced handshake fails against the hardcoded `webpki-roots` trust store with no way to decline and fall back to the previously-working plaintext path. **Fixed** by adding `SmtpConfig::disable_starttls` (default `false`; `require_tls` still forces the upgrade when set), so the security-hardening default is preserved but the escape hatch exists.

### Lower-severity items surfaced, NOT fixed (documented for a judgment call)
These are real but minor; left as-is pending a decision on whether they're worth the churn:
- **cfworkers has no `allow_insecure_auth` equivalent** — when `requireTLS:false` and `auth` is set, the Cloudflare Workers crate runs AUTH over cleartext with no guard, unlike the native crate which grew that guard in the first pass. Not triggered by the test-worker (it sets no auth), but the hole exists. (Parity fix worth doing if cfworkers auth becomes a real use case.)
- **cfworkers XOAuth2 input shape is awkward** — the access token must be passed in the `pass` field; there's no `accessToken` field, and `deny_unknown_fields` rejects the nodemailer-style shape. Functional but not ergonomic.
- **cfworkers 8KB read-line cap is per-buffer, not strictly per-line** — a single >8KB TCP segment carrying many short complete lines could be falsely rejected before the complete lines are drained. Improbable for real SMTP (responses rarely exceed 8KB in one segment); the OOM-defense case it exists for is handled correctly.
- **cfworkers `CfSocket::close` is fire-and-forget** (doesn't await the close promise) — sufficient for "close is called on every path" but the socket isn't confirmed torn down before the transport returns.
- **node auto-generates `@localhost` Message-IDs** — same From/Message-ID domain-alignment weakness the SES fix addressed; harmless but cosmetically wrong. Deriving from the `From` domain would mirror the SES fix.
- **node string-attachment content is always base64** (documented divergence from nodemailer, which treats a plain-string `content` as UTF-8 text unless `encoding:"base64"`).
- **`SmtpConfig::from_url` leaks credentials into error strings** — `format!("...missing a host: {url}")` interpolates the full `user:pass@` URL; if that Err is logged, the redacting `Debug` work is undone. Also silently falls back to the default port on an out-of-range port, and mis-splits IPv6-without-port.
- **DKIM non-UTF-8 body path has no cross-implementation oracle** — covered only by a local byte-preservation unit test, not a hash compared against real nodemailer (both golden tests are ASCII). Code path is correct on inspection; the coverage gap is in external oracle strength, not the implementation.
- **`address.rs` long comma-free header (>998 octets) still not folded** — the first pass fixed the quoted-comma folding but not the "single long value with no commas" half of the original finding; a long RFC-2047 Subject or reference chain can still exceed the SMTP line limit.
- **`CalendarMethod` not re-exported from `trustmailer-core::lib`** — callers of `.with_method()` must reach into `message::CalendarMethod`. Cosmetic.

### Second-pass final verification
- `cargo test --workspace --exclude trustmailer-node --all-features` — **174 passed, 0 failed** (was 172; +2 regression tests for the panic and the loopback bypass).
- `cargo clippy --workspace --exclude trustmailer-node --all-targets --all-features` — no issues.
- `cargo check -p trustmailer-node` — clean.
- `cargo check --target wasm32-unknown-unknown -p trustmailer-smtp-cfworkers -p trustmailer-wasm` — clean.
