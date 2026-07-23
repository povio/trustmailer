---
title: Addresses
description: How trustmailer parses, groups, renders, and hardens address fields — from a single mailbox to nested RFC 5322 groups.
---

Every address field on a [Message](/docs/message/) — `from`, `to`, `cc`, `bcc`,
`reply_to` — is built from the same `Address` type and the same parser, ported from
nodemailer's `addressparser`. That parser is deliberately **infallible**: real inboxes
are full of malformed, half-quoted, comment-laden address text, and a mail library that
rejects it outright is less useful than one that degrades gracefully, the same way a
mail client's own "To" field has to. `parse_address_list` never returns an error — worst
case, a malformed entry becomes an `Address` with an empty `name` or `address` rather
than being dropped or panicking.

## The `Address` shape

```rust
pub struct Address {
    pub name: String,
    pub address: String,
    pub group: Option<Vec<Address>>,
}
```

An `Address` is either a single mailbox (`group` is `None`) or an RFC 5322 group
(`group` is `Some(members)`, `address` is empty, `name` is the group label). Construct
one directly when you already have structured data:

```rust
use trustmailer::address::Address;

let plain = Address::new("bob@example.com");
let named = Address::with_name("Bob Smith", "bob@example.com");
let group = Address::group("Sales Team", vec![
    Address::new("alice@example.com"),
    Address::new("carol@example.com"),
]);
```

## Accepted string formats

`to`, `cc`, `bcc`, and `reply_to` all accept anything implementing `IntoAddresses`,
which covers a `&str`/`String` (parsed as an address list), a single `Address`, a
`Vec<Address>`, or a `Vec<&str>` (each string parsed independently and concatenated).
As plain text, the parser recognizes the same shapes nodemailer does:

| Input | Parsed as |
|---|---|
| `"bob@example.com"` | `Address { name: "", address: "bob@example.com" }` |
| `"Bob Smith <bob@example.com>"` | `Address { name: "Bob Smith", address: "bob@example.com" }` |
| `"Bob Smith bob@example.com"` (no angle brackets) | Same as above — an address is detected inside free text even without `<...>` |
| `"bob@example.com (Bob Smith)"` | Same as above — a trailing parenthetical comment is used as the display name when no other name is present |
| `"\"Smith, Bob\" <bob@example.com>"` | `Address { name: "Smith, Bob", address: "bob@example.com" }` — a quoted display name may contain a comma or semicolon without it being read as a field separator |
| `"bob@example.com, carol@example.com"` or `"bob@example.com; carol@example.com"` | Two entries — both `,` and `;` are accepted top-level separators |

```rust
use trustmailer::Message;

let message = Message::builder()
    .from("Alice <alice@example.com>")
    .to("Bob Smith <bob@example.com>, \"Carol, PhD\" <carol@example.com>")
    .build();
```

:::tip[Quote display names that contain a comma]
`"Joe Foo, PhD <joe@example.com>"` (unquoted) is still recovered correctly — the parser
recombines a run of name-only fragments followed by one with both a name and an address —
but this is a best-effort heuristic, not a guarantee for every edge case. Wrapping the
display name in double quotes, `"\"Joe Foo, PhD\" <joe@example.com>"`, is the reliable
way to include a comma and is what trustmailer itself always emits when rendering a
name back out (see [Rendering back to a header](#rendering-back-to-a-header)).
:::

## Address groups (RFC 5322)

A group names a list of recipients as one unit — `Group-Name: a@x.com, b@y.com;` — and
is parsed into an `Address` with `group: Some(...)`:

```rust
use trustmailer::address::parse_address_list;

let parsed = parse_address_list("Sales:alice@example.com, carol@example.com;");
assert!(parsed[0].is_group());
```

An empty group (`"Undisclosed:;"`) parses to a group with zero members — trustmailer's
equivalent of nodemailer's undisclosed-recipients idiom. Groups can nest
(`"Outer:Inner:deep@example.com;;"`), and `parse_address_list_flatten` (nodemailer's
`{ flatten: true }`) expands every group into its leaf mailboxes, dropping the group
structure entirely:

```rust
use trustmailer::address::parse_address_list_flatten;

let flat = parse_address_list_flatten("Group1:a@b.com, Group2:c@d.com;;");
// -> [Address { address: "a@b.com", .. }, Address { address: "c@d.com", .. }]
```

The SMTP envelope has no concept of a group — `Message::effective_envelope()` always
flattens `to`/`cc`/`bcc` before deriving `RCPT TO`, so a group in any of those fields
still reaches every one of its members. See [Message](/docs/message/#envelope-and-effective_envelope)
for the full envelope derivation.

## Limits and hardening

Because address fields are attacker-reachable input (a `To:`/`Cc:` header on inbound
mail, or a value forwarded from an untrusted upstream), the parser bounds its own work
rather than trusting its input to be well-formed or bounded in size:

:::caution[Two independent caps]
- **`MAX_ADDRESS_LIST_ENTRIES` (4096)** — a single address-list string is only parsed up
  to this many top-level (comma/semicolon-separated) entries; anything past the cap is
  truncated rather than fully parsed, bounding CPU/memory against a maliciously huge
  `To`/`Cc` value.
- **`MAX_NESTED_GROUP_DEPTH` (50)** — RFC 5322 doesn't allow nested groups at all, so
  anything nested deeper than this is truncated. This cap is enforced in two independent
  places: in the text parser itself, *and* in `Address::flatten`/`Address::to_header_value`
  — the latter guards against a directly-constructed (not parsed) `Address` graph, e.g.
  one deserialized from untrusted JSON via `Address`'s `serde` support, which could
  otherwise recurse without bound and risk a stack overflow even though it never went
  through the text parser at all.

Both caps truncate silently rather than returning an error, consistent with the parser's
overall infallible-by-design contract. See [Security](/docs/security/) for the rest of
the hardening ported from nodemailer's own regression tests, including the header
injection fix these caps complement.
:::

A few narrower parsing details worth knowing:

- **Domain literals** — `user@[IPv6:2001:db8::1]` keeps the colon inside the `[...]`
  bracket rather than misreading it as a group separator; an unclosed `[` doesn't swallow
  the rest of the list (`"alice@x.com, bob[@x.com, carol@x.com"` still yields three
  separate addresses).
- **Line-ending normalization** — a bare `\r` (not part of a `\r\n` pair) inside address
  text normalizes to a space, exactly like a bare `\n` does, rather than being silently
  dropped.
- **Quoted local parts** — `"user@attacker.com"@legitimate.com` is parsed as one address
  whose local part happens to contain an `@`, not misread as `user@attacker.com` with a
  stray trailing domain; the parser never lets a quoted local-part fool the "detect a
  bare address inside free text" heuristic into picking the wrong `@`.

## Rendering back to a header

`Address::to_header_value()` (used internally whenever a `From`/`To`/`Cc`/`Reply-To`
header is written) renders a mailbox as `Name <address>` or a group as
`Name: member, member;`, applying RFC 5322/2047 rules on the way out:

- A non-ASCII display name is wrapped as an RFC 2047 encoded-word:
  `=?UTF-8?B?<base64>?=`.
- An ASCII display name containing `,`, `;`, `<`, `>`, or `"` is quoted, with any literal
  `\` or `"` inside it escaped (`\` first, then `"` — escaping in the other order would
  double-escape backslashes just inserted and could leave the quoted string
  unterminated).
- A plain ASCII name with none of those characters is emitted bare.

This is also the path every custom or `list` header value goes through before being
written to the wire — see [Message → Header options](/docs/message/#header-options) for
the shared CRLF-injection hardening applied there.

## From Node.js

`MailOptions.from`/`to`/`cc`/`bcc`/`replyTo` are plain strings, parsed with the exact
same rules described above — `to: "bob@example.com,carol@example.com"` behaves
identically to the Rust `.to("bob@example.com,carol@example.com")` call, since the
napi binding forwards the string straight into the same builder method:

```js
const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "Bob Smith <bob@example.com>, \"Carol, PhD\" <carol@example.com>",
  subject: "Hello",
  text: "Hi!",
});
```

:::note[No structured address object in the Node binding yet]
The current `MailOptions` only accepts address fields as strings — there's no
`{ name, address }` object form exposed from Node yet (that's a plain Rust `Address`
struct on the core side). If you need to build a display name programmatically from
Node, compose the string yourself (quoting it if it contains a comma) rather than
relying on an object literal.
:::

See [Message](/docs/message/) for the rest of the message model, and
[Attachments](/docs/attachments/) for how attachment `cid`s (a related but distinct
identifier, not a mail address) let HTML reference an embedded image.
