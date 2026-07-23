---
title: DKIM signing
description: How trustmailer signs outgoing mail with DKIM, and exactly what's been verified about it.
---

DKIM (DomainKeys Identified Mail, RFC 6376) lets a receiving mail server confirm that a
message really was sent by whoever it claims to be from, and that nothing in it was
altered in transit. trustmailer signs with RSA-SHA256 over relaxed/relaxed
canonicalization — the same combination nodemailer defaults to — implemented in the
`trustmailer-dkim` crate as a small, self-contained signer plus a `Transport` decorator
that wraps whichever transport you're already using.

Because this is the one part of the codebase whose whole job is cryptographic
correctness, [`AUDIT.md`](https://github.com/nesgarbo/trustmailer/blob/main/AUDIT.md)
treats it as higher-priority than similarly-tagged findings elsewhere in the project, and
it went through two independent audit passes. The callouts below are drawn from that
audit, not from marketing copy — where something was found and fixed, it says so.

## How signing works

Signing happens in four steps, all inside `DkimSigner::sign`:

1. **Split** the raw RFC 5322 message into headers and body, unfolding any header
   continuation lines along the way.
2. **Canonicalize** both, using the *relaxed* algorithm from RFC 6376 §3.4.2/§3.4.4:
   header names are lowercased, runs of spaces/tabs collapse to one space, and
   leading/trailing whitespace is trimmed; the body has trailing whitespace stripped from
   each line and trailing blank lines dropped, always ending in exactly one CRLF.
3. **Hash** the canonical body with SHA-256 to get the `bh=` tag, then build the full
   signing input (canonical signed headers + the DKIM-Signature header itself, value
   still missing `b=`) and hash *that*.
4. **Sign** that second hash with RSA-PKCS#1v1.5, base64-encode the signature into `b=`,
   and prepend the finished `DKIM-Signature:` header to the message.

:::note[Byte-level, not text-level]
Both canonicalization steps operate directly on the message's raw bytes — there's no
`String`/`char` decoding step anywhere in the path. That matters for anything with a
legacy 8-bit body (`iso-8859-1`/`windows-1252` text, a raw/binary transfer encoding):
lossily decoding through UTF-8 first would silently mutate those bytes before hashing,
which produces a `bh=` that doesn't match what the receiving server computes over the
real wire bytes — a signature that verifies as broken with no error ever shown to the
sender. An earlier version of this crate did decode through
`String::from_utf8_lossy` before hashing; it was found and rewritten to operate on raw
bytes end to end, and the fix was hand-traced against RFC 6376 §3.4.4 across every edge
case (empty body, no trailing newline, interior blank lines, bare CR, non-UTF-8 bytes,
folded/padded headers) in a second, independent verification pass.
:::

## Quick start (native Rust)

```rust
use trustmailer::{Message, Transport};
use trustmailer::dkim::{DkimSigner, SigningTransport};
use trustmailer::smtp::{SmtpConfig, SmtpTransport};

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let private_key_pem = std::fs::read_to_string("dkim-private.pem")?;
    let signer = DkimSigner::from_pkcs8_pem("example.com", "default", &private_key_pem)?;

    let smtp = SmtpTransport::new(SmtpConfig::for_service("gmail").unwrap());
    let transport = SigningTransport::new(signer, smtp);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .build();

    let info = transport.send(&message).await?;
    println!("{:?}", info);
    Ok(())
}
```

`SigningTransport` implements the same `Transport` trait as everything else, so it
composes with *any* inner transport — SMTP, sendmail, stream, JSON — and the caller on
the other side of it never has to know signing is happening.

## Quick start (Node.js)

`trustmailer-node`'s `createTransport` takes a `dkim` option shaped like nodemailer's,
and wires a `SigningTransport` in front of whichever transport you configured:

```js
import { createTransport } from "trustmailer";
import { readFileSync } from "node:fs";

const transporter = createTransport({
  service: "gmail",
  auth: { user: "user@gmail.com", pass: "app-password" },
  dkim: {
    domainName: "example.com",
    keySelector: "default",
    privateKey: readFileSync("dkim-private.pem", "utf8"),
  },
});

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});
```

## Configuration reference

`DkimSigner` is built with a constructor plus a small number of builder methods —
there's no separate "options object", so each row below maps to a Rust call (and, where
noted, a corresponding JS field on the Node binding's `dkim` option):

| Rust                                             | Node `dkim` field | Required | Purpose |
| ------------------------------------------------- | ----------------- | :------: | ------- |
| `DkimSigner::from_pkcs8_pem(domain, selector, pem)` | —                 | yes      | Constructs the signer. `domain` becomes the `d=` tag, `selector` becomes `s=`, `pem` must be a PKCS#8 PEM-encoded RSA private key. |
| — (first arg above)                               | `domainName`       | yes      | The signing domain, published in the `d=` tag and looked up as `<selector>._domainkey.<domain>` for the public key. |
| — (second arg above)                              | `keySelector`      | yes      | The DKIM selector, published in the `s=` tag. |
| — (third arg above)                               | `privateKey`       | yes      | PEM-encoded PKCS#8 private key text. |
| `.with_headers(Vec<String>)`                       | — (not yet exposed)| no       | Replaces the default list of headers to sign (see below). Case-insensitive; lowercased internally. |
| `.key_bits()`                                      | —                  | no       | Informational — returns the RSA key size in bits, e.g. to warn on sub-1024-bit keys. |
| `SigningTransport::new(signer, inner)`             | (implied by setting `dkim`) | yes | Wraps `inner` so every `send()` signs first. |
| `.with_hostname(String)`                           | — (not yet exposed)| no       | Hostname used when generating a `Message-ID` for messages that don't set one. Defaults to `"localhost"`. |

## Choosing which headers get signed

By default, `DkimSigner` signs whichever of a fixed, RFC 4871 §5.5-recommended list of
headers are actually present on the message — `From`, `Sender`, `Reply-To`, `Subject`,
`Date`, `Message-ID`, `To`, `Cc`, MIME/content headers, `Resent-*`, `In-Reply-To`,
`References`, and the `List-*` headers — always signed in that fixed priority order
rather than whatever order they appear in the message, so two signers given the same key
and message produce byte-identical signatures.

Call `.with_headers([...])` to replace that list outright with your own — it's an
**explicit sign-list**, not an exclude-list: whatever you pass is exactly what gets
looked for and signed, in the order you give it. If a header you name isn't present on
the message it's simply skipped, not an error. If a message has more than one header
with the same name, the header nearest the body is the one that gets signed.

:::caution[`From` must be in the signed set]
`sign()` fails with `Error::Dkim("From header must be signed")` if `From` doesn't end up
among the signed headers — whether because the message has no `From` header at all, or
because a custom `.with_headers([...])` list left it out. This is a deliberate, separate
check from the general "at least one header got signed" check: a signature that covers
some header but not `From` would validate at the receiver while saying nothing
trustworthy about who actually sent the message — the exact property DKIM exists to
guarantee. An earlier version of this crate only checked for "signed set non-empty",
which a `.with_headers([...])` call omitting `"from"` could pass while leaving the
sender identity itself unsigned; that gap was found and closed by adding this dedicated
check.
:::

## Domain and selector are validated, not trusted blindly

`DkimSigner::from_pkcs8_pem` rejects any `domain` or `selector` containing a CR, LF,
`;`, or `"` before it ever touches key parsing:

```rust
// All of these return Err(Error::Dkim(...)) instead of constructing a signer:
DkimSigner::from_pkcs8_pem("example.com\r\nX-Injected: evil", "default", pem);
DkimSigner::from_pkcs8_pem("example.com", "default\"", pem);
DkimSigner::from_pkcs8_pem("example.com;d=evil.com", "default", pem);
```

:::caution[This closed a real header-injection gap]
Every other header value in trustmailer's MIME layer is sanitized through one
central choke-point before being written to the wire. `domain`/`selector` were the one
path that bypassed it: they're interpolated directly into the `DKIM-Signature` value via
`format!`, so an unvalidated CRLF in either would have let a caller inject arbitrary
headers (or terminate the header and inject body content) into every DKIM-signed
message — reachable in practice from `trustmailer-node`'s `dkim.domainName`/
`dkim.keySelector`, i.e. straight from untrusted-ish JS-side config. It was found
independently by two separate audit passes looking at the DKIM crate and the Node
facade from different angles, and fixed at the root: validation now happens once, in
the constructor, rather than trying to route the interpolated value back through the
general-purpose header sanitizer. `;` and `"` are rejected too, since either could
terminate the tag-value early inside the signature itself. Both the Rust crate and the
Node binding surface the rejection as a real, typed error rather than a generic "invalid
key" message.
:::

## Line folding

Large keys (2048/4096-bit, the sizes anyone should actually use in production) produce a
`b=` value long enough to push the generated `DKIM-Signature` header past RFC 5322's
998-octet line cap. trustmailer folds the header at `;` tag boundaries — never in the
middle of a tag's value — inserting `"\r\n\t"` once a line would otherwise grow past 76
columns. This is safe by construction: relaxed header canonicalization unfolds
(`CRLF` + whitespace collapses back to a single space) before hashing, on both the
signing and verifying side, so folding changes nothing about what actually gets signed
or verified.

## Performance: signing doesn't block your async runtime

RSA-2048 signing takes low-single-digit milliseconds, not microseconds — cheap in
absolute terms, but expensive relative to how fast an async executor expects a `poll()`
to return. `SigningTransport::send` runs the actual signing call inside
`tokio::task::spawn_blocking` on native targets, so it can't stall whatever shared Tokio
worker thread happens to be polling it and starve other concurrent sends. On `wasm32`
there's no blocking thread pool to offload to (and the runtime is single-threaded
anyway), so signing runs inline there instead — the same behavior, just without the
offload, since there's nothing else to protect.

## Cross-checked against real nodemailer output

`trustmailer-dkim` includes a golden test that signs a fixed message with a fixed key and
asserts the resulting `bh=` and `b=` match, byte for byte, output independently captured
from real nodemailer's own DKIM signer given the identical key and message. RSA-SHA256
under PKCS#1v1.5 is deterministic, so this is a meaningfully stronger check than "the
output looks like a valid signature" — it confirms trustmailer and nodemailer compute the
literal same signature from the same inputs. The relaxed-body canonicalization step has
its own separate golden test ported directly from nodemailer's `relaxed-body-test.js`
fixture.

## What's deliberately not here (yet)

A few things nodemailer's DKIM support offers that trustmailer's source doesn't
currently implement — noted here rather than left to be discovered by trial and error:

- **No key rotation / multiple simultaneous keys.** nodemailer accepts a `keys` array so
  a message can be signed with more than one key (e.g. during a selector migration).
  `DkimSigner` holds exactly one key; running multiple `SigningTransport`s in sequence
  isn't equivalent, since only the outermost `DKIM-Signature` ends up covering the
  others.
- **No `hashAlgo` choice.** Signing is always RSA-SHA256; there's no option to pick a
  different digest algorithm.
- **No disk-backed caching for huge messages** (nodemailer's `cacheDir`/
  `cacheTreshold`). trustmailer builds and hashes the canonicalized message in memory.
- **No per-message DKIM override.** Signing is configured once, at
  `SigningTransport::new`/`createTransport({ dkim: {...} })` time, for every message that
  transport sends — there's no per-`sendMail()`-call `dkim` field.
- **Not yet wired through the browser/WASM or Cloudflare Workers JS bindings.** At the
  Rust level, `SigningTransport<T>` is generic over any `Transport` and has a dedicated
  inline (non-`spawn_blocking`) signing path specifically for `target_arch = "wasm32"`,
  so `wasm32` support was clearly intended. In practice, though, actually building
  `trustmailer-dkim` for `wasm32-unknown-unknown` currently runs into a known,
  pre-existing dependency gap: the `rsa` crate's transitive `rand` → `getrandom`
  dependency needs a WASM-entropy feature enabled that nothing in the dependency graph
  currently requests. Neither `trustmailer-wasm` nor `trustmailer-smtp-cfworkers`
  depends on `trustmailer-dkim`/`rsa` at all, so they're unaffected by that gap — but
  neither one's `createTransport()` currently exposes a `dkim` option the way the Node
  binding does anyway. Only `trustmailer-node` wires DKIM up today.

See [Security](/docs/security/) for the full picture of what's been audited across the
rest of the codebase, and the [runtime matrix](/docs/runtime-matrix/) for where DKIM
signing is (and isn't yet) reachable per runtime.
