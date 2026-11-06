// Cross-runtime smoke test for the native addon: same .node binary, no rebuild,
// no per-runtime code. Run under whichever runtime you want to verify:
//   node smoke-test.cjs
//   bun smoke-test.cjs
//   deno run -A smoke-test.cjs   (Deno needs the .cjs extension to use require())
const { createTransport } = require("./index.js");

const runtime = typeof Bun !== "undefined" ? `bun ${Bun.version}` : typeof Deno !== "undefined" ? `deno ${Deno.version.deno}` : `node ${process.version}`;

const transporter = createTransport({ streamTransport: true });
transporter
  .sendMail({
    from: "Alice <alice@example.com>",
    to: "bob@example.com",
    subject: "Runtime check",
    text: `hello from ${runtime}`,
  })
  .then((info) => {
    console.log(`OK (${runtime}): rendered ${Buffer.from(info.rawBase64, "base64").length} bytes, accepted=${JSON.stringify(info.accepted)}`);
  })
  .catch((err) => {
    console.error(`FAIL (${runtime}):`, err);
    process.exit(1);
  });
