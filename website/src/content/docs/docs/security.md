---
title: Security
description: What was found, what was fixed, and how it was verified — twice.
---

The seal in the mark isn't decoration. trustmailer's whole pitch is nodemailer's model
without having to trust a JS mail stack's transitive dependency tree — so the trust has to
be earned in the injection-prone parts: header assembly, envelope addresses, and DKIM
signing.

## Found by porting nodemailer's own regression tests

Two vulnerability classes were found and fixed by porting nodemailer's own security
regression tests, rather than just its happy-path tests — the kind of bug that only shows
up when you deliberately go looking for the attacker's-eye view:

**MIME header injection** — ported from `test/nodemailer/list-headers-test.js`, regression
test for [GHSA-268h-hp4c-crq3](https://github.com/advisories/GHSA-268h-hp4c-crq3): any
header value containing a CRLF could terminate that header early and inject arbitrary
follow-on headers into the message. Fixed with universal sanitization at the single point
every header is written (`mime/node.rs::write_header_line`), not per call site.

**SMTP command injection** — ported from `test/smtp-connection/smtp-connection-test.js`:
the `EHLO` name and `MAIL FROM`/`RCPT TO` envelope addresses are written directly into SMTP
protocol commands. A CRLF in the EHLO name is stripped at assignment
(`SmtpConfig::with_ehlo_name`); a CRLF/`<`/`>` in an envelope address is rejected before any
bytes reach the socket (`SmtpConnection::send_transaction`).

## Found by actually running it

A third bug, this one not from porting a test but from running the Cloudflare Workers path
live end-to-end: `sendMail()`'s JS return value came back as an empty `{}` in both
`trustmailer-wasm` and `trustmailer-smtp-cfworkers`. `serde-wasm-bindgen` serializes struct
fields as a JS `Map` by default, and a `Map` has no own enumerable properties, so
`Response.json()`/`JSON.stringify()` silently rendered it as `{}` — no error, just
quietly-wrong data reaching the caller. Fixed by serializing with
`serialize_maps_as_objects(true)` in both crates. A docs-only review or a compile-only
check would never have surfaced this — it only showed up because the actual JSON response
was inspected end-to-end against a real `workerd` run.

## Independently re-audited

After the first fix pass, a second, independent audit re-verified every fix and went
looking specifically for what the first pass missed — regressions the fixes themselves
might have introduced, and anything both passes overlooked. It found three more issues,
all now fixed and regression-tested:

- A **reachable parser panic**: a comment-only address list entry (`(bar)` with no
  preceding address) desynchronized two parallel internal vectors and indexed out of
  bounds — reachable from any `To:`/`Cc:` value, violating the address parser's
  infallible-by-design contract.
- A **bypassable OAuth2 credential guard**: the HTTPS-only restriction on the OAuth2 token
  endpoint had a loopback exception (`http://127.0.0.1...`) implemented as a bare
  `starts_with`, so `http://127.0.0.1.evil.com` slipped through and would have sent
  `client_secret`/`refresh_token` in cleartext to an attacker-controlled host.
- A **STARTTLS regression**: an earlier fix made STARTTLS upgrade unconditional whenever a
  server advertised it, with no way to decline — breaking connections to internal relays
  presenting an untrusted certificate that previously worked in plaintext. A
  `disable_starttls` escape hatch was added back.

## Verified, not assumed

- **DKIM canonicalization** signs over raw bytes, not lossily-decoded text, and is checked
  against real nodemailer's own signer: the same RSA key and message produce
  byte-identical `bh=` and `b=` values — a golden cross-implementation test, not just an
  internal assertion.
- **174 tests across 23 suites**, `cargo clippy --workspace --all-targets` clean, verified
  building on native, `wasm32-unknown-unknown`, and the Node.js native addon.
- The Cloudflare Workers SMTP path is verified against a real `wrangler dev`/`workerd`
  run — see the [Cloudflare Workers quickstart](/docs/quickstart/cloudflare-workers/) for
  the reproduction steps.

The full, unabridged audit — every finding, every file:line, every fix, including the
lower-severity items left open on purpose — is public in
[`AUDIT.md`](https://github.com/nesgarbo/trustmailer/blob/main/AUDIT.md).
