# trustmailer

Rust-powered, [nodemailer](https://nodemailer.com)-compatible mail library — a native
addon via [napi-rs](https://napi.rs). One compiled binary, same API, runs unmodified on
Node.js, Bun, and Deno.

Full documentation, transport reference, and a real comparison against nodemailer:
**[trustmailer.nesgarbo.com](https://trustmailer.nesgarbo.com)**

## Install

```sh
npm install trustmailer
```

Prebuilt binaries ship for macOS (x64/arm64), Linux (x64/arm64, glibc/musl), and Windows
(x64/arm64) via `optionalDependencies` — no Rust toolchain needed at install time.

## Usage

```js
const { createTransport } = require("trustmailer");

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
- [Source](https://github.com/nesgarbo/trustmailer)

## License

MIT OR Apache-2.0

## Render an EML draft (Node.js / Bun)

```js
const { renderEmlBase64 } = require('trustmailer');
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

Regression checks after `npm install && npm run build`: `npm test` and
`npm run test:bun` (the MIME checks use Python 3's independent `email` parser).
See [render memory evidence](../../benchmarks/render-memory.md) for reproduction and limits.
