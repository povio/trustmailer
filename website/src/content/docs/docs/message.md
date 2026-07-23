---
title: Message
description: Every field on trustmailer's Message/MessageBuilder — content, routing, headers, and how it becomes a MIME byte stream.
---

A `Message` is trustmailer's equivalent of nodemailer's `Mail` options object: one plain
Rust struct (`trustmailer_core::Message`) holding everything about an email — sender,
recipients, body, attachments, and headers — before it's handed to a `Transport`. You
normally never build a `Message` by hand; you go through `Message::builder()`, a fluent
`MessageBuilder` that mirrors nodemailer's field names wherever the concept exists in
Rust too. The same model is exposed from Node.js as `MailOptions`, from WASM/browser
hosts through the same core crate, and from Cloudflare Workers via
`trustmailer-smtp-cfworkers` — see [Architecture](/docs/architecture/) for how one
`trustmailer-core` crate backs every runtime.

```rust
use trustmailer::Message;

let message = Message::builder()
    .from("Alice <alice@example.com>")
    .to("bob@example.com")
    .cc(vec!["carol@example.com", "dave@example.com"])
    .subject("Hello")
    .text("Hi Bob!")
    .html("<b>Hi Bob!</b>")
    .build();
```

From Node.js, the same message is a plain object passed to `sendMail`:

```js
const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  cc: "carol@example.com,dave@example.com",
  subject: "Hello",
  text: "Hi Bob!",
  html: "<b>Hi Bob!</b>",
});
```

:::note[Node bindings expose a subset]
`trustmailer-node`'s `MailOptions` currently covers `from`, `to`, `cc`, `bcc`, `replyTo`,
`subject`, `text`, `html`, `messageId`, and `attachments` (see
`crates/trustmailer-node/src/lib.rs`). Fields further down this page —
`watchHtml`/`ampHtml`, custom headers, `priority`, `envelope`, `icalEvent`, and the
`list`/`listUnsubscribe` family — are implemented in `trustmailer-core` and available to
every native Rust and WASM caller, but aren't wired into the napi binding yet. If you need
one of them from Node today, build the raw MIME with the core crate directly instead of
going through `MailOptions`.
:::

## Common fields

| Field | Builder method | Type | Description |
|---|---|---|---|
| `from` | `.from(addr)` | address | Sender shown in the `From:` header. Also the default envelope-from (`MAIL FROM`) unless overridden — see [Envelope](#envelope-and-effective_envelope) below. |
| `to` | `.to(addrs)` | address list | Primary recipients, `To:` header. |
| `cc` | `.cc(addrs)` | address list | Carbon-copy recipients, `Cc:` header — visible to every other recipient. |
| `bcc` | `.bcc(addrs)` | address list | Blind carbon-copy recipients. Used to build the SMTP envelope but **never** written to any header — confirmed by trustmailer's own test suite, which asserts a `Bcc:` line never appears in rendered output. |
| `subject` | `.subject(s)` | string | `Subject:` header. Automatically RFC 2047-encoded if it contains non-ASCII text. |
| `text` | `.text(s)` | string | Plain-text body, rendered as a `text/plain; charset=utf-8` part, quoted-printable encoded. |
| `html` | `.html(s)` | string | HTML body, rendered as a `text/html; charset=utf-8` part. |
| `attachments` | `.attachment(a)` (repeatable) | `Attachment` | Files and inline images — see [Attachments](/docs/attachments/). |

`to`, `cc`, `bcc`, and `reply_to` all accept anything implementing `IntoAddresses`: a
single `&str`/`String` (parsed as a comma/semicolon-separated address list), an
`Address`, a `Vec<Address>`, or a `Vec<&str>` (each entry parsed independently). See
[Addresses](/docs/addresses/) for the full parsing rules — quoted display names,
comments-as-name, RFC 5322 groups, and the safety caps on pathological input.

## Content fields

Beyond `text`/`html`, trustmailer supports the same three extra content channels
nodemailer does, each rendered as its own MIME alternative:

| Field | Builder method | Rendered as | Purpose |
|---|---|---|---|
| `watch_html` | `.watch_html(s)` | `text/watch-html; charset=utf-8` | A simplified HTML fallback for clients that strip full HTML but tolerate light markup (historically Apple Watch). |
| `amp_html` | `.amp_html(s)` | `text/x-amp-html; charset=utf-8` | An AMP4EMAIL interactive body, for clients that support it. |
| `ical_event` | `.ical_event(event)` | `text/calendar; charset=utf-8; method=...` | A calendar invite/update — see [Calendar invites](#calendar-invites) below. |

When more than one content part is present, they're wrapped in a `multipart/alternative`
container in a fixed order: `text` → `watch_html` → `amp_html` → `html` → any extra
`alternatives` → `ical_event`. Mail clients pick the last alternative they understand how
to render, which is why the richest part (HTML, then the calendar invite) comes last.

```rust
let message = Message::builder()
    .text("Plain-text fallback.")
    .watch_html("<b>Simplified for small screens</b>")
    .html("<h1>Full HTML</h1><p>...</p>")
    .build();
```

:::tip[No content at all still sends]
If neither `text` nor `html` (nor any other content field) is set, trustmailer falls
back to a single empty `text/plain` part rather than producing an invalid message —
useful for attachment-only notifications.
:::

### Calendar invites

`ical_event` takes a `CalendarEvent`, built from raw `.ics` content:

```rust
use trustmailer::message::{CalendarEvent, CalendarMethod};

let event = CalendarEvent::new(ics_body).with_method(CalendarMethod::Request);

let message = Message::builder()
    .to("bob@example.com")
    .subject("Meeting invite")
    .text("See attached invite.")
    .ical_event(event)
    .build();
```

`CalendarMethod` is a 4-value enum (`Publish` — the default, `Request`, `Reply`,
`Cancel`) matching RFC 5546's `METHOD` property exactly; it's an enum rather than a raw
string specifically so an arbitrary value can never be interpolated into the
`Content-Type: text/calendar; method=...` parameter. `CalendarEvent`'s default filename
is `invite.ics`.

## Routing and envelope

| Field | Builder method | Notes |
|---|---|---|
| `reply_to` | `.reply_to(addrs)` | `Reply-To:` header — where replies should go if different from `from`. |
| `envelope` | `.envelope(env)` | Explicit SMTP envelope (`MAIL FROM` / `RCPT TO`), overriding the one trustmailer would otherwise derive. See below. |
| `sender`, `in_reply_to`, `references`, `date` | *(no builder method yet)* | Set directly on the built `Message` — its fields are all `pub`. See [Fields without a builder method](#fields-without-a-builder-method-yet). |

### Envelope and `effective_envelope()`

The SMTP envelope (who the message is actually delivered to/from at the protocol level)
can legitimately differ from the visible `From`/`To` headers — for example, VERP bounce
handling uses a `MAIL FROM` that encodes the original recipient. Call `.envelope(...)`
to set it explicitly; otherwise, `Message::effective_envelope()` derives one the same way
nodemailer does:

- **from**: `sender` if set, else `from`. If neither is set, this returns
  `Error::NoSender`.
- **to**: `to` + `cc` + `bcc`, flattened (any address groups are expanded to their member
  mailboxes, since an SMTP envelope has no notion of a group), deduplicated (preserving
  original order — not sorted, since envelope order can matter to a receiving server),
  with empty addresses filtered out. If the result is empty, this returns
  `Error::NoRecipients`.

```rust
use trustmailer::error::Error;

match message.effective_envelope() {
    Ok(envelope) => { /* envelope.from, envelope.to */ }
    Err(Error::NoSender) => eprintln!("message has no from/sender address"),
    Err(Error::NoRecipients) => eprintln!("message has no to/cc/bcc recipients"),
    Err(e) => eprintln!("unexpected: {e}"),
}
```

:::caution[BCC reaches the envelope, never the headers]
`bcc` recipients are included in `effective_envelope().to` (so the message is actually
delivered to them) but are never written into any rendered header — that's the entire
point of a blind copy. Don't rely on `bcc` addresses appearing anywhere in the raw MIME.
:::

## Header options

| Field | Builder method | Notes |
|---|---|---|
| `message_id` | `.message_id(id)` | Explicit `Message-ID:` value. If omitted, one is auto-generated as `<uuid@hostname>` at build time (or, via the Node binding, pre-generated before sending so it's reported back on the result). |
| `priority` | `.priority(p)` | `Priority::High`/`Normal`/`Low`. Sets both `X-Priority` (`1 (Highest)` / `3 (Normal)` / `5 (Lowest)`) and `Importance` (`high`/`normal`/`low`) headers together. |
| `headers` | `.header(name, value)` (repeatable) | Arbitrary extra header, appended verbatim (RFC 2047-encoded automatically if non-ASCII). |
| `list` | `.list(key, entries)`, `.list_unsubscribe(entries)`, `.list_unsubscribe_post()` | `List-*` headers — see below. |

Every header value, regardless of source — subject, a custom `.header(...)`, a `list`
comment, an attachment header — passes through the same serialization path
(`mime/node.rs::write_header_line`), which collapses any embedded `\r`/`\n` to a single
space before the header is written. A malicious or buggy value can't terminate a header
early and inject a forged follow-on header (see [Security](/docs/security/)). Long
header lines are also folded at ~76 columns per RFC 5322 §2.2.3, splitting only on commas
outside quoted strings so a comma inside a quoted display name is never folded on.

### `List-*` headers

`.list(key, entries)` sets a `List-<Key>` header (`"unsubscribe"` → `List-Unsubscribe`,
`"id"` → `List-ID`, `"help"`, `"subscribe"`, `"post"`, `"owner"`, `"archive"`, or any
other suffix you choose — any key is accepted, matching nodemailer's `list` option).
Each entry is a `ListEntry`, a URL plus an optional human-readable comment:

```rust
use trustmailer::message::ListEntry;

let message = Message::builder()
    .from("news@example.com")
    .to("subscriber@example.com")
    .subject("Newsletter")
    .text("...")
    .list_unsubscribe(vec![
        ListEntry::new("https://example.com/unsub?id=123").with_comment("One-click"),
        ListEntry::new("mailto:unsub@example.com"),
    ])
    .list_unsubscribe_post()
    .build();
```

`list_unsubscribe(entries)` is shorthand for `.list("unsubscribe", entries)`.
`list_unsubscribe_post()` adds `List-Unsubscribe-Post: List-Unsubscribe=One-Click` per
RFC 8058 — Gmail and Yahoo's bulk-sender rules require this alongside a working
`List-Unsubscribe` URL. A bare `user@host` value is wrapped as `<mailto:user@host>`; a
`http(s)`/`ftp`/`mailto` URL is wrapped as `<scheme:...>`; anything else falls back to
`<http://...>`. `List-ID` is special-cased to the RFC 2919 bare `<domain>` form instead
of `<scheme:url>`.

## Fields without a builder method yet

`Message`'s fields are all `pub`, and `MessageBuilder::build()` hands you a plain
`Message` — so a few fields that don't (yet) have a fluent builder method can still be
set directly on the value `build()` returns:

```rust
let mut message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .text("...")
    .build();

message.sender = Some("alice-bounces@example.com".into()); // Sender: header
message.in_reply_to = Some("<previous@example.com>".into()); // In-Reply-To:
message.references = vec!["<thread-root@example.com>".into()]; // References:
message.date = Some(time::OffsetDateTime::now_utc()); // override the auto Date:
message.raw = Some(prebuilt_mime_bytes); // bypass the builder entirely — see below
```

- **`sender`** — `Sender:` header, distinct from `From:` (e.g. someone sending on behalf
  of another address). Also used as the envelope-from ahead of `from`, if set.
- **`in_reply_to`** / **`references`** — threading headers.
- **`date`** — overrides the auto-generated `Date:` header (RFC 5322 format, UTC
  `now()` by default). Its type is `Option<time::OffsetDateTime>` from the
  [`time`](https://docs.rs/time) crate — trustmailer doesn't re-export it, so setting this
  field directly requires adding `time = "0.3"` to your own `Cargo.toml` too.
- **`alternatives`** — extra top-level MIME parts beyond `text`/`html`/`watch_html`/
  `amp_html` (e.g. a `text/calendar` part you built yourself instead of via
  `ical_event`); push onto `message.alternatives` directly.
- **`raw`** — a fully pre-built RFC 5322 byte stream. When set, the MIME builder returns
  it verbatim and ignores every other field on the message — trustmailer's equivalent of
  nodemailer's `raw` option, for callers who already have a complete message to send
  as-is.

## How it becomes MIME

`build(&message, hostname)` in `trustmailer-core`'s MIME builder assembles the parts
above into an RFC 5322 byte stream:

1. Every present content field becomes a leaf node, in the fixed order described in
   [Content fields](#content-fields). A single part needs no wrapper; two or more are
   wrapped in `multipart/alternative`.
2. Inline attachments (`content_disposition: inline` **and** a `cid` set) are wrapped
   together with that body in `multipart/related`, so `<img src="cid:...">` references
   resolve inside the same client-rendered unit.
3. Any remaining (non-inline, or inline-without-`cid`) attachments wrap the result again
   in `multipart/mixed`.
4. Root headers (`From`, `To`, `Cc`, `Reply-To`, `Subject`, `Date`, `Message-ID`, `List-*`,
   custom headers, ...) are prepended last. `Bcc` is deliberately never among them.

See [Attachments](/docs/attachments/) for the full attachment/embedded-image story, and
[Addresses](/docs/addresses/) for how every address field above is parsed and rendered.
