// Real throughput comparison: build+render N messages through each library's own
// jsonTransport, called the same way a real Node.js app would (`sendMail()`), timed with
// `process.hrtime.bigint()`. Not a marketing number — run `npm run bench` yourself.
//
// This measures "how fast can each library build and render a message when called from
// Node.js" — it does NOT depend on trustmailer-node's jsonTransport response-content gap
// (see parity.cjs and the comparison page), since timing doesn't need the content back.
"use strict";

const nodemailer = require("nodemailer");
const { createTransport } = require("trustmailer");

const N = Number(process.argv[2]) || 20000;
const WARMUP = Math.min(1000, Math.floor(N / 10));

function mail(i) {
  return {
    from: "Alice <alice@example.com>",
    to: `Bob <bob${i}@example.com>, carol${i}@example.com`,
    cc: `dave${i}@example.com`,
    subject: `Benchmark message #${i}`,
    text: `Hi Bob, this is message number ${i}. Lorem ipsum dolor sit amet, consectetur adipiscing elit.`,
    html: `<p>Hi Bob, this is message number <b>${i}</b>.</p><p>Lorem ipsum dolor sit amet, consectetur adipiscing elit.</p>`,
  };
}

async function timeIt(label, fn, n) {
  for (let i = 0; i < WARMUP; i++) await fn(i);
  const start = process.hrtime.bigint();
  for (let i = 0; i < n; i++) await fn(i);
  const end = process.hrtime.bigint();
  const ms = Number(end - start) / 1e6;
  const perSec = (n / ms) * 1000;
  return { label, n, ms: Math.round(ms), perSec: Math.round(perSec) };
}

async function main() {
  const nmTransport = nodemailer.createTransport({ jsonTransport: true });
  const tmTransport = createTransport({ jsonTransport: true });

  const nmResult = await timeIt(
    "nodemailer (jsonTransport)",
    (i) => nmTransport.sendMail(mail(i)),
    N
  );
  const tmResult = await timeIt(
    "trustmailer (jsonTransport)",
    (i) => tmTransport.sendMail(mail(i)),
    N
  );

  console.log(
    JSON.stringify(
      {
        node: process.version,
        platform: `${process.platform}/${process.arch}`,
        nodemailerVersion: require("nodemailer/package.json").version,
        trustmailerVersion: require("trustmailer/package.json").version,
        messagesPerRun: N,
        warmupMessages: WARMUP,
        results: [nmResult, tmResult],
        ratio: `trustmailer is ${(tmResult.perSec / nmResult.perSec).toFixed(2)}x nodemailer's msgs/sec`,
      },
      null,
      2
    )
  );
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
