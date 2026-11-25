# @povio/trustmailer

Rust-powered, [nodemailer](https://nodemailer.com)-compatible mail library — a native
addon via [napi-rs](https://napi.rs). One compiled binary, same API, runs unmodified on
Node.js, Bun, and Deno.

Full documentation, transport reference, and a real comparison against nodemailer:
**[trustmailer.nesgarbo.com](https://trustmailer.nesgarbo.com)**

## Install

```sh
npm install @povio/trustmailer
```

Native binaries are bundled directly in this package; no Rust toolchain is needed
at install time. The first local release (`0.1.1`) contains only the Apple Silicon
macOS binary. Subsequent releases built by GitHub Actions include macOS (x64/arm64),
Linux (x64/arm64, glibc/musl), and Windows (x64/arm64).

This is the Povio fork. See [PUBLISHING.md](PUBLISHING.md) for local publishing and
GitHub Actions trusted publishing setup.

## Usage

```js
const { createTransport } = require("@povio/trustmailer");

const transporter = createTransport({
  host: "smtp.example.com",
  port: 587,
  auth: { user: "user", pass: "pass" },
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

Same code runs under Bun (`bun run`) and Deno (`deno run -A`, needs a `.cjs`/`.mjs`
extension or a `package.json` with `"type"` set) — it's the same native addon underneath.

## Links

- [Docs & transport reference](https://trustmailer.nesgarbo.com/docs/)
- [nodemailer vs trustmailer comparison](https://trustmailer.nesgarbo.com/docs/comparison/)
- [Source](https://github.com/povio/trustmailer)

## License

MIT OR Apache-2.0

## Render an EML draft (Node.js / Bun)

```js
const { renderEmlBase64 } = require('@povio/trustmailer');
const base64 = renderEmlBase64({
  // Sender and recipient strings may be omitted or blank for a draft.
  subject: 'Draft – 日本語',
  to: '"Doe, Jane" <jane@example.com>, bob@example.com',
  text: 'Hello',
  html: '<b>Hello</b>',
  headers: { 'X-Unsent': '1' },
  attachments: [{ filename: 'data.bin', content: Buffer.from([0, 255]) }],
});
// Decode only if you need a file; base64 can be passed straight to a download API.
require('node:fs').writeFileSync('draft.eml', Buffer.from(base64, 'base64'));
```

`renderEmlBase64(options: MailOptions): string` synchronously calls the core MIME
builder without SMTP envelope validation or delivery. It does not add `X-Unsent`
automatically and does not sign with DKIM. Recipient fields retain the existing
comma-separated string API; quote display names containing commas. Header values
are encoded/sanitized by the core builder; invalid custom header names throw.
Attachment strings are base64; Buffers are bytes.

`sendMail()` keeps transport validation and delivery semantics. Stream transport
returns its status in `response` and exact MIME bytes in the new `rawBase64` field;
use `renderEmlBase64()` for drafts with blank envelopes.

Rendering returns a Rust String instead of an external native output Buffer, so
large native MIME allocations are released before the JavaScript result is returned.
The native addon uses mimalloc (without process allocator override features); WASM
and Rust library consumers keep their own allocators. Allocator licensing is in
`LICENSE-MIMALLOC`. Release artifacts must remain unstripped: stripped Rust 1.94
Mach-O libraries were rejected by the macOS 27 loader with a LINKEDIT alignment error.

## Stream an EML draft with large attachments (Node.js / Bun / Deno)

`renderEmlStream` renders the same MIME as `renderEml`, but reads each attachment from
a stream while the output is consumed, so attachments are never all in memory.

```js
const { renderEmlStream } = require('@povio/trustmailer');
const { Readable } = require('node:stream');

const eml = await renderEmlStream({
  subject: 'Draft',
  to: 'bob@example.com',
  html: '<b>Hello</b>',
  headers: { 'X-Unsent': '1' },
  attachments: [
    // `open` runs when the part is about to be written; return null to leave it out.
    { filename: 'report.pdf', contentType: 'application/pdf', open: () => openFromS3(key) },
    { filename: 'small.txt', content: Buffer.from('in memory is fine too') },
  ],
});
Readable.fromWeb(eml).pipe(res); // or `new Response(eml)` on Bun/Deno/fetch servers
```

`renderEmlStream(options): Promise<ReadableStream<Uint8Array>>` accepts the same
options as `renderEml`. An attachment may have `open` instead of `content`; `open()`
returns (or resolves to) a web `ReadableStream`, an async iterable of `Uint8Array`
(such as a Node.js `Readable`), a `Uint8Array`, or `null`.

- Headers, boundaries, RFC 2047/2231 encoding and header sanitizing come from the same
  native builder as `renderEml`. For the same input the bytes differ only in `Date`,
  a generated `Message-ID`, and the random boundaries. Bodies are base64 with
  76-character CRLF lines, whatever the source's chunk sizes.
- Sources open in the order their parts appear (inline `cid` attachments first). The
  first opens before the promise resolves, so `open` errors there reject it, and a server
  can still answer with an HTTP error. Each later source opens when the previous one
  starts streaming, so at most one is open ahead.
- `open()` returning `null` drops that attachment's whole MIME part. A rejected
  `open()`, a source error, or a chunk that is not a `Uint8Array` errors the stream.
- The stream pulls lazily (high-water mark 0). Cancelling it cancels the source being
  read and the one opened ahead (a Node.js `Readable` is destroyed).

Custom `headers` are written in the object's key order.

## Building and tests

`npm run build` writes the napi-rs loader to `binding.js`/`binding.d.ts`. `index.js` and
`index.d.ts` are maintained by hand and add `renderEmlStream`. Do not run
`napi build --platform` without `--js binding.js`, or it overwrites `index.js`.

Regression checks after `npm install && npm run build`: `npm test` and
`npm run test:bun` (the MIME checks use Python 3's independent `email` parser).
See [render memory evidence](../../benchmarks/render-memory.md) for reproduction and limits.
