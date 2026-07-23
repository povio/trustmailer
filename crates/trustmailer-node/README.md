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
