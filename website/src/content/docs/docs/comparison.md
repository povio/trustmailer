---
title: nodemailer vs trustmailer
description: A real, reproducible comparison — output parity, a CRLF-injection regression test, throughput, and nodemailer's current published advisories.
---

Every claim on this page comes from a script in the [`benchmarks/`](https://github.com/nesgarbo/trustmailer/tree/main/benchmarks)
directory of the repo, run against the real, currently-published `nodemailer` package —
not a vendored copy, not an old version chosen to look bad. Clone the repo and run
`cd benchmarks && npm install && npm run parity && npm run bench` to get your own
numbers; they'll vary by hardware, but the shape of the result won't.

## Security: nodemailer's current advisories

Running `npm install nodemailer` today (this page was last regenerated against
**nodemailer 9.0.3**, the latest release) and then `npm audit` surfaces six currently
open, high-severity advisories affecting nodemailer `<=9.0.0` — fixed only in the just-released
9.0.1+ line:

| Advisory | Issue |
| --- | --- |
| [GHSA-c7w3-x93f-qmm8](https://github.com/advisories/GHSA-c7w3-x93f-qmm8) | SMTP command injection via unsanitized `envelope.size` |
| [GHSA-vvjj-xcjg-gr5g](https://github.com/advisories/GHSA-vvjj-xcjg-gr5g) | SMTP command injection via CRLF in the transport name (EHLO/HELO) |
| [GHSA-268h-hp4c-crq3](https://github.com/advisories/GHSA-268h-hp4c-crq3) | CRLF injection in `List-*` header comments — arbitrary header injection |
| [GHSA-wqvq-jvpq-h66f](https://github.com/advisories/GHSA-wqvq-jvpq-h66f) | `jsonTransport` bypasses `disableFileAccess`/`disableUrlAccess` during normalization |
| [GHSA-r7g4-qg5f-qqm2](https://github.com/advisories/GHSA-r7g4-qg5f-qqm2) | Improper TLS certificate validation during OAuth2 token fetch |
| [GHSA-p6gq-j5cr-w38f](https://github.com/advisories/GHSA-p6gq-j5cr-w38f) | Message-level `raw` option bypasses file/URL-access restrictions — arbitrary file read and SSRF |

That third one, GHSA-268h-hp4c-crq3, is the exact CRLF-header-injection class
trustmailer ported as a regression test from nodemailer's own `test/` suite (see
[Security](/docs/security/)) — the same bug class, found independently, on both sides of
this comparison.

:::note[This isn't "nodemailer is bad"]
Nodemailer fixed all six in the 9.0.1+ line — if you're on a current install, you're
fine. The point isn't that nodemailer is poorly maintained; it's that hand-written
SMTP-command and MIME-header assembly in a dynamically-typed language has produced six
real advisories across its history, which is precisely the class of bug a Rust core with
explicit sanitization at a single choke point (`mime/node.rs::write_header_line`) is
harder to reintroduce by accident. Whether that tradeoff matters to you is a real
judgment call, not a foregone conclusion — this page just puts real numbers next to it.
:::

## Output parity

[`parity.cjs`](https://github.com/nesgarbo/trustmailer/blob/main/benchmarks/parity.cjs)
builds the identical message (`from`/`to`/`cc`/`subject`/`text`/`html`) through both
libraries and checks the parts that should genuinely match — recipient count, subject,
text, and HTML bodies come back identical, verified field by field, not eyeballed.

It also runs the GHSA-268h-hp4c-crq3 CRLF-injection payload through **both real libraries'
real code paths** and confirms neither forges a second header — but only through the
transport that actually builds MIME headers (`streamTransport` on both sides).
`jsonTransport` doesn't sanitize CRLF on **either** library, because neither one builds
real headers there — it just serializes the raw field values for inspection. That's not
a trustmailer-specific gap; it's confirmed identical behavior on both sides, and the
script asserts on that too rather than hiding it.

```
7/7 checks passed
```

One asymmetry the script had to work around, and is worth stating plainly: as of this
writing, `trustmailer-node`'s `sendMail()` doesn't return the actual rendered
JSON/MIME content for `jsonTransport`/`streamTransport` the way nodemailer's
`info.message` does — only a fixed confirmation string (see
[Stream](/docs/transports/stream/) and [JSON](/docs/transports/json/) transport docs).
`parity.cjs` routes around this by calling the real Rust crate directly for
trustmailer's side of the comparison. This is a real, open gap in the Node.js binding,
not swept under the rug.

## Throughput

[`bench.cjs`](https://github.com/nesgarbo/trustmailer/blob/main/benchmarks/bench.cjs)
builds and renders the same batch of messages (`jsonTransport`, so no network is
involved on either side) through each library's real `sendMail()`, timed with
`process.hrtime.bigint()`, after a warmup pass. Three separate runs of 20,000 messages
each, on the same machine, back to back:

| Run | nodemailer | trustmailer | Ratio |
| --- | ---: | ---: | ---: |
| 1 | 10,159 msg/s | 15,767 msg/s | 1.55× |
| 2 | 9,866 msg/s | 20,819 msg/s | 2.11× |
| 3 | 9,908 msg/s | 15,718 msg/s | 1.59× |

Roughly **1.5–2×** nodemailer's throughput for building and rendering a message,
consistently across runs — with real run-to-run variance, reported honestly rather than
cherry-picking the best number. This measures message construction/rendering only (no
network I/O either side); it says nothing about actual SMTP delivery throughput, which is
dominated by the remote server and network, not the client library.

## Install footprint

Also measured directly, not estimated:

| | nodemailer | trustmailer (Node.js) |
| --- | --- | --- |
| Runtime npm dependencies | **0** | **0** |
| On-disk size | 676 KB (plain JS) | ~20 MB (one compiled native binary) |

Both are honestly dependency-free at the npm level — this was one of the few claims in
this comparison that turned out to be a wash rather than a win, and it's reported that
way. The size difference isn't a supply-chain risk on either side; it's a distribution
model difference. nodemailer ships interpreted JS with no bundled crypto/TLS/HTTP stack
(it defers TLS to Node's own `tls` module). trustmailer ships one compiled binary that
statically links its own TLS (`rustls`), HTTP client (`reqwest`), and RSA/SHA (`rsa`,
`sha2`) implementations — the tradeoff for the memory-safety and the specific CVE classes
above, not a free lunch.

## What's not compared here

- **Feature parity** in general — see [Why](/docs/why/) and [Message](/docs/message/)
  for what's ported and what's deliberately not (DSN, `attachDataUrls`, custom auth
  plugins, and more — see each page's own callouts).
- **Multi-runtime support** — nodemailer is Node-only by design; this page is a
  same-runtime comparison. See the [runtime support matrix](/docs/runtime-matrix/) for
  what trustmailer adds beyond Node.js.
- **Real SMTP delivery performance** — both libraries' actual send throughput is bound by
  the destination server and network, not measured here.

## Reproduce it yourself

```sh
git clone https://github.com/nesgarbo/trustmailer
cd trustmailer
cargo build --release --example render_json --example render_stream -p trustmailer
cd crates/trustmailer-node && npx napi build --platform --release && cd ../..
cd benchmarks
npm install
npm run parity
npm run bench
```
