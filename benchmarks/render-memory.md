# Draft rendering verification

Run on 2026-11-04 on macOS 27.2 ARM64, Rust 1.94.0, release build with
`strip = false`, mimalloc 0.1.52 / libmimalloc-sys 0.1.49. No global environment
settings were changed. The built addon loaded in Node 24.21.0 and Bun 1.4.4.

From the repository root:

```sh
cargo test --workspace --locked
cd crates/trustmailer-node
npm ci
npm run build
npm test
bun run test:bun
cd ../..
node benchmarks/render-memory.cjs 300 10
bun benchmarks/render-memory.cjs 300 10
```

Python 3 is required for the regression test's independent standard-library MIME
parser. Tests cover missing/blank draft envelopes, Unicode subject/display name/
filename/body, quoted commas in recipient lists, HTML + text, binary/base64/empty/
multiple attachments, header and filename injection, separate stream status/MIME,
and SMTP/sendmail rejection of missing sender/recipients before external I/O.
The Rust workspace suite, including fake SMTP tests, passed locally.

The memory script reuses a preallocated 10 MiB input Buffer, constructs actual
multipart MIME and base64 on every call, discards each output, and records RSS
without forced GC. Results are saved alongside this document:

- `render-memory-node-darwin-arm64.json`: 300 calls, peak RSS 135.5 MiB.
- `render-memory-bun-darwin-arm64.json`: 300 calls, peak RSS 226.2 MiB.

Samples are instantaneous RSS after each 25th call; peak RSS comes from
`process.resourceUsage().maxRSS` (KiB). These are single runs, not medians or a
comparison against Nodemailer/the old allocator. They show this implementation
finishing the repeated-render workload without the previously reported multi-GiB
growth; they do not establish a universal memory bound. Retaining JS outputs,
concurrent calls, and attachment size/content change memory requirements.

Historical Shippy evaluation (provided context, **not rerun here**) reported a
macOS Bun median peak reduction from 1093.3 to 256.4 MiB with mimalloc (Nodemailer
639.8 MiB), and Linux ARM64 final stress RSS about 207–245 MiB. Those used the
private binding and different harnesses, so they are not direct comparisons to
these new upstream runs. This machine cannot verify Linux/Windows, Intel macOS,
Deno, older Node versions, or WASM runtime loading. Existing target packaging is
retained; native release CI now also runs the MIME regression test where runnable.
No package has been published and no Shippy code or PR was modified.
