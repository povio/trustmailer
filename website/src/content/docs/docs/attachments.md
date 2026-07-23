---
title: Attachments
description: Attaching files and embedding inline images with trustmailer — content sources, filename encoding, transfer encoding, and the URL-fetch SSRF caveat.
---

Attachments are files carried inside a [Message](/docs/message/), each one a
`trustmailer_core::Attachment` — mirroring nodemailer's `attachments` array. An
attachment can be plain bytes you already have in memory, a base64 string, a path on
disk, or a remote URL fetched at send time; the same struct also covers **embedded
images** (inline content referenced from HTML via `cid:`) by way of one extra field.

```rust
use trustmailer::{Message, attachment::Attachment};

let message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .subject("Report attached")
    .text("See attached.")
    .attachment(Attachment::from_bytes("report.pdf", pdf_bytes).with_content_type("application/pdf"))
    .build();
```

Call `.attachment(...)` once per file — it appends, so multiple calls (or a loop)
attach multiple files to the same message.

## Attachment fields

| Field | Type | Description |
|---|---|---|
| `filename` | `Option<String>` | Name shown to the recipient, and used to auto-detect a MIME type if `content_type` isn't set. `None` omits the filename parameter entirely. |
| `content` | `AttachmentContent` | Where the bytes come from — see [Content sources](#content-sources). |
| `content_type` | `Option<String>` | Explicit MIME type. If unset, it's guessed from `filename`'s extension, falling back to `application/octet-stream`. |
| `content_disposition` | `ContentDisposition` | `Attachment` (default, downloadable) or `Inline` (rendered in place — used for embedded images). |
| `cid` | `Option<String>` | Content-ID for referencing this attachment from HTML as `cid:<value>`. Set together with an `Inline` disposition by `.inline_with_cid(...)`. |
| `headers` | `Vec<(String, String)>` | Extra MIME headers on this attachment's own MIME part, appended verbatim. |

## Content sources

`AttachmentContent` is an enum with four variants, matching where nodemailer lets an
attachment's bytes come from:

| Variant | Availability | Notes |
|---|---|---|
| `Bytes(Vec<u8>)` | Everywhere, including WASM | Bytes you already have in memory. |
| `Base64(String)` | Everywhere | A base64-encoded payload, decoded when the attachment is resolved — equivalent to nodemailer's `encoding: "base64"`. |
| `Path(PathBuf)` | Native only, behind the `fs` Cargo feature | Read from local disk with `std::fs::read`. There's no filesystem in a WASM host, so this variant doesn't exist there at all when `fs` is disabled. |
| `Url(String)` | Behind the `http` Cargo feature | A remote URL, fetched over HTTP(S) — see [Fetching from a URL](#fetching-from-a-url) below. |

```rust
use trustmailer::attachment::{Attachment, AttachmentContent};

// From bytes already in memory:
let a1 = Attachment::from_bytes("data.csv", csv_bytes);

// From a local file (native + `fs` feature):
let a2 = Attachment {
    content: AttachmentContent::Path("/tmp/report.pdf".into()),
    ..Attachment::from_bytes("report.pdf", Vec::new())
};
```

Calling `attachment.resolve_bytes()` on a `Url` variant that hasn't been fetched yet
returns an error telling you to call `resolve_all()` first — it never blocks
synchronously to fetch over the network.

### Content type detection

`resolved_content_type()` returns, in order: the explicit `content_type` if set;
otherwise a guess from `filename`'s extension (via the `mime_guess` crate); otherwise
`application/octet-stream`. This also decides transfer encoding — see below.

### Transfer encoding

Every attachment part is encoded before being embedded in the message: content whose
resolved MIME type starts with `text/` is quoted-printable encoded, and everything else
(images, PDFs, archives, ...) is base64 encoded. This is decided automatically from the
content type; there's currently no field to force a specific transfer encoding.

## Filenames with unicode or special characters

The `filename=`/`name=` MIME parameter is rendered two different ways depending on what
the filename contains:

- A **plain ASCII** filename with no literal `"` or control character is emitted as a
  simple quoted string: `filename="report.pdf"`.
- Anything else — non-ASCII text (`"ÕÄÖÜ.txt"`), or ASCII containing a literal `"` that
  can't be safely embedded in a quoted string (`"\"test\" it is.txt"`) — is emitted in
  the RFC 2231 extended form: `filename*=UTF-8''%C3%95%C3%84...`.

Both `Content-Type`'s `name=` parameter and `Content-Disposition`'s `filename=` parameter
use the same rule.

:::note[No `name*0*=` continuation chunking]
Unlike nodemailer, trustmailer doesn't split a very long RFC 2231 value into
`name*0*=`/`name*1*=` continuation pieces — every mainstream mail client parses a
single-chunk `name*=UTF-8''...` value fine, and chunking mainly matters for filenames
past roughly 100 characters.
:::

## Embedded images (`cid`)

To show an image inline in HTML rather than as a downloadable attachment, attach it
with `.inline_with_cid(id)`, which sets `content_disposition: Inline` and the given
`cid` in one call, then reference it from the HTML body as `cid:<id>`:

```rust
let message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .subject("Photo")
    .html(r#"<p>Look:</p><img src="cid:logo123">"#)
    .attachment(
        Attachment::from_bytes("logo.png", logo_bytes)
            .with_content_type("image/png")
            .inline_with_cid("logo123"),
    )
    .build();
```

Attach as many inline images as you like, each with its own unique `cid` matching an
`<img src="cid:...">` reference in the HTML.

:::tip[Only attachments that are both inline *and* have a `cid` get grouped this way]
When building the MIME tree, trustmailer partitions attachments into those with
`content_disposition: Inline` **and** a `cid` set, versus everything else. Only the
first group is wrapped together with the message body in a `multipart/related`
container (so mail clients know to resolve `cid:` references against them); everything
else is wrapped separately in `multipart/mixed` as ordinary downloadable attachments. An
attachment marked `Inline` without a `cid` — or any attachment when the message has no
HTML body to reference it from — is not pulled into `multipart/related`.
:::

Give a `cid` a domain-like, collision-resistant value (e.g. `logo@example.com`) rather
than something generic like `img1` if you're building messages programmatically at
scale — nothing enforces uniqueness for you.

## Fetching from a URL

`Attachment::from_url(filename, url)` (behind the `http` Cargo feature) defers fetching
until you call `resolve_all(&mut message)`, an async function that walks every `Url`
attachment (and `alternatives` entry) on the message and replaces it with downloaded
bytes in place — call it once, before `build()`/`send()`:

```rust
use trustmailer::attachment::{resolve_all, Attachment};

let mut message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .subject("Logo")
    .text("See attached.")
    .attachment(Attachment::from_url("logo.png", "https://example.com/logo.png"))
    .build();

resolve_all(&mut message).await?;
let info = transport.send(&message).await?;
```

`resolve_all` works from a browser/WASM host too — on `wasm32`, the underlying HTTP
client goes through `fetch` rather than a raw socket.

:::caution[25MB size cap, and no SSRF hardening]
Fetched responses are capped at **25MB** (`MAX_FETCH_URL_BYTES`) — checked against
`Content-Length` up front when the server sends one, and enforced again while streaming
the body in chunks, so a response with a missing or dishonest `Content-Length` still
can't grow past the cap. A response over the limit, or a non-2xx status, fails
`resolve_all` with an error rather than silently truncating.

This size cap is the *only* protection `resolve_all` applies. It does **not** perform
any SSRF hardening — no private/link-local IP blocking, no scheme or destination
allowlist. If the URL you pass to `from_url` can be influenced by an untrusted party
(e.g. a user-submitted "avatar URL"), fetching it is exactly as dangerous as any other
server-side fetch of untrusted input, and restricting which URLs are safe to fetch is
your application's responsibility, not trustmailer's. See [Security](/docs/security/)
for the rest of trustmailer's threat model.
:::

## Custom attachment headers

`headers: Vec<(String, String)>` adds arbitrary extra MIME headers to a single
attachment's part, appended after the standard `Content-Type`/`Content-Transfer-Encoding`/
`Content-Disposition`/`Content-ID` headers:

```rust
let attachment = Attachment {
    headers: vec![("X-Attachment-Source".to_string(), "internal-report-service".to_string())],
    ..Attachment::from_bytes("report.pdf", pdf_bytes)
};
```

Every header trustmailer writes — attachment-level or message-level — passes through the
same CRLF-stripping serialization path described in [Message → Header options](/docs/message/#header-options),
so an attacker-controlled header value here can't inject a forged follow-on header
either. Unlike `Message::header(name, value)`, though, attachment headers aren't
automatically RFC 2047-encoded for non-ASCII values — keep custom attachment header
values ASCII.

## From Node.js

`MailOptions.attachments` is an array of `AttachmentInput` objects:

```js
const info = await transporter.sendMail({
  from: "alice@example.com",
  to: "bob@example.com",
  subject: "Report",
  text: "See attached.",
  html: '<img src="cid:logo123">',
  attachments: [
    { filename: "report.pdf", content: pdfBuffer, contentType: "application/pdf" },
    { filename: "data.csv", content: base64Csv }, // base64 string works too
    { filename: "logo.png", content: logoBuffer, contentType: "image/png", cid: "logo123" },
  ],
});
```

| Field | Type | Notes |
|---|---|---|
| `filename` | `string \| undefined` | Shown to the recipient. |
| `content` | `Buffer \| string` | A `Buffer`, or a base64-encoded string — either works, matching `Bytes`/`Base64` on the Rust side. |
| `contentType` | `string \| undefined` | Explicit MIME type; guessed from `filename` if omitted. |
| `cid` | `string \| undefined` | Setting this automatically switches the attachment to an inline disposition — you don't set `contentDisposition` separately. |

:::note[Path/URL attachments and per-attachment headers aren't in the Node binding yet]
`AttachmentInput` only accepts in-memory content (a `Buffer` or a base64 string) — there's
no `path`/`url` field wired into `trustmailer-node` today, and no `headers` field for
per-attachment MIME headers either. Both exist on the Rust core's `Attachment` type (see
above); if you need them from a Node.js process, the current option is to fetch/read the
file yourself in JS and pass the resulting `Buffer`, or build the message with the core
crate directly.
:::

See [Message](/docs/message/) for how attachments fit into the overall MIME structure,
and [Addresses](/docs/addresses/) for how the rest of the message's fields are parsed.
