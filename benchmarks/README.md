# trustmailer vs nodemailer — real comparison scripts

Backs the [comparison page](https://trustmailer.nesgarbo.com/docs/comparison/) on the
docs site. Every number there came from running these scripts — nothing here is
hand-tuned to make a point; if a number looks unflattering, it's reported anyway.

## Setup

```sh
cd trustmailer  # repo root
cargo build --release --example render_json --example render_stream -p trustmailer
cd crates/trustmailer-node && npx napi build --platform --release && cd ../..
cd benchmarks
npm install
```

## Scripts

- `npm run parity` — builds an identical message through both libraries and checks that
  the parts that should match do (recipients, subject, text, HTML), then runs the
  GHSA-268h-hp4c-crq3 CRLF-injection payload through both libraries' real MIME-building
  code path and confirms neither forges a second header.
- `npm run bench [count]` — builds and renders `count` messages (default 20,000) through
  each library's real `sendMail()` via `jsonTransport` (no network involved), timed with
  `process.hrtime.bigint()` after a warmup pass, and reports messages/sec for each.

`parity.cjs` calls the compiled `render_json`/`render_stream` Rust examples directly for
trustmailer's side, rather than going through `trustmailer-node`'s `sendMail()` — as of
this writing, the Node binding's `jsonTransport`/`streamTransport` only return a fixed
confirmation string, not the rendered content, unlike nodemailer's `info.message`. That
gap is real; see the comparison page.

## Why this exists as a real project, not just claims in prose

trustmailer's whole pitch is "verified, not assumed" (see the repo's `AUDIT.md`). A
comparison page that just asserts "faster" or "more secure" without a script anyone can
run and get real numbers from would be exactly the kind of unverified marketing claim
this project is trying not to be.
