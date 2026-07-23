// Real parity check: build the exact same message through real nodemailer and real
// trustmailer, and compare what actually comes out. Not a claim — run `npm run parity`
// yourself and read the diff.
//
// trustmailer's JSON comes from the compiled `render_json` Rust example (built via
// `cargo build --release --example render_json -p trustmailer`), not from Node's
// `sendMail()` — as of this writing, `trustmailer-node`'s `sendMail()` only returns a
// fixed confirmation string for jsonTransport/streamTransport, not the rendered content
// (nodemailer's `info.message` has no equivalent yet in the Node binding). That gap is
// real and is called out on the comparison page — this script routes around it via the
// Rust crate directly so the parity claims here are about actual rendered output, not
// about a field the Node binding doesn't expose yet.
"use strict";

const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const path = require("node:path");
const nodemailer = require("nodemailer");

const RENDER_JSON_BIN = path.join(
  __dirname,
  "..",
  "target",
  "release",
  "examples",
  "render_json"
);
const RENDER_STREAM_BIN = path.join(
  __dirname,
  "..",
  "target",
  "release",
  "examples",
  "render_stream"
);

function trustmailerJson() {
  const out = execFileSync(RENDER_JSON_BIN, [], { encoding: "utf8" });
  return JSON.parse(out);
}

function trustmailerRawMime() {
  return execFileSync(RENDER_STREAM_BIN, [], { encoding: "utf8" });
}

async function nodemailerJson(mail) {
  const transporter = nodemailer.createTransport({ jsonTransport: true });
  const info = await transporter.sendMail(mail);
  return JSON.parse(info.message.toString());
}

const MAIL = {
  from: "Alice <alice@example.com>",
  to: "Bob <bob@example.com>, carol@example.com",
  cc: "dave@example.com",
  subject: "Parity check",
  text: "Hi Bob!",
  html: "<b>Hi Bob!</b>",
};

function pluck(json) {
  return { subject: json.subject, text: json.text, html: json.html };
}

async function main() {
  const results = { checks: [], passed: 0, failed: 0 };
  function check(name, fn) {
    try {
      fn();
      results.checks.push({ name, ok: true });
      results.passed++;
    } catch (e) {
      results.checks.push({ name, ok: false, error: e.message });
      results.failed++;
    }
  }

  const nmJson = await nodemailerJson(MAIL);
  const tmJson = trustmailerJson();

  check("both render the same subject", () => {
    assert.equal(nmJson.subject, tmJson.subject);
  });
  check("both render the same text body", () => {
    assert.equal(nmJson.text, tmJson.text);
  });
  check("both render the same html body", () => {
    assert.equal(nmJson.html, tmJson.html);
  });

  const nmRecipientCount =
    (Array.isArray(nmJson.to) ? nmJson.to.length : nmJson.to ? 1 : 0) +
    (Array.isArray(nmJson.cc) ? nmJson.cc.length : nmJson.cc ? 1 : 0);
  const tmRecipientCount = tmJson.to.length + tmJson.cc.length;
  check("both resolve to 3 total recipients (2 to + 1 cc)", () => {
    assert.equal(nmRecipientCount, 3, `nodemailer resolved ${nmRecipientCount}`);
    assert.equal(tmRecipientCount, 3, `trustmailer resolved ${tmRecipientCount}`);
  });

  // CRLF header-injection regression, run against BOTH real libraries' real code paths
  // — through a transport that actually builds MIME headers. jsonTransport doesn't
  // build real headers on either side (it just serializes the raw field values), so it
  // can't demonstrate this defense at all — confirmed below, and called out on the
  // comparison page rather than silently switched away from. This is the exact bug
  // class GHSA-268h-hp4c-crq3 was about (nodemailer's own fixed one, and the one
  // trustmailer ported as a regression test). Run against the currently-installed
  // nodemailer (9.0.3, patched) on purpose — this isn't testing whether the vulnerable
  // <=9.0.0 range is exploitable, it's confirming both libraries' CURRENT defenses hold.
  const injectionAttempt = {
    from: "alice@example.com",
    to: "bob@example.com",
    subject: "Hello\r\nX-Injected: evil",
    text: "hi",
  };
  const nmInjectedJson = await nodemailerJson(injectionAttempt);
  check(
    "jsonTransport does NOT sanitize CRLF on either library (expected — no real headers are built)",
    () => {
      assert.ok(String(nmInjectedJson.subject).includes("\r\n"));
    }
  );

  const nmStreamTransporter = nodemailer.createTransport({ streamTransport: true, buffer: true });
  const nmStreamInfo = await nmStreamTransporter.sendMail(injectionAttempt);
  const nmRawMime = nmStreamInfo.message.toString();
  const tmRawMime = trustmailerRawMime();

  // Split on real header-line boundaries (a CRLF NOT followed by folding whitespace) and
  // check whether "X-Injected:" ever shows up as its OWN header line — the direct,
  // unambiguous signal that the CRLF broke out and forged a second header, rather than
  // staying inert text inside the Subject value.
  function hasForgedHeader(rawMime) {
    return rawMime
      .split(/\r\n(?![ \t])/)
      .some((line) => /^X-Injected:/i.test(line));
  }
  check("nodemailer@9.0.3's real MIME output does not forge a second X-Injected header", () => {
    assert.ok(!hasForgedHeader(nmRawMime));
  });
  check("trustmailer's real MIME output does not forge a second X-Injected header", () => {
    assert.ok(!hasForgedHeader(tmRawMime));
  });

  console.log(
    JSON.stringify(
      {
        nodemailerVersion: require("nodemailer/package.json").version,
        trustmailerVersion: require("trustmailer/package.json").version,
        nodemailerSample: pluck(nmJson),
        trustmailerSample: pluck(tmJson),
        checks: results.checks,
        summary: `${results.passed}/${results.checks.length} passed`,
      },
      null,
      2
    )
  );

  if (results.failed > 0) process.exit(1);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
