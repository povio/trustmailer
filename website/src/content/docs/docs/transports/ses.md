---
title: Amazon SES transport
description: Delivering mail through the SES v2 HTTPS API with a hand-rolled SigV4 signer — no AWS SDK dependency.
---

The SES transport delivers your composed message through Amazon Simple Email Service's
v2 `SendEmail` HTTPS API, authenticated with AWS Signature Version 4. It's implemented
in `trustmailer-ses`, re-exported from the facade crate as `trustmailer::ses` behind the
`ses` Cargo feature (on by default).

The distinguishing choice here is that trustmailer signs the request itself — there is
no `aws-sdk-sesv2` / `aws-config` dependency anywhere in the crate. SigV4 is a documented,
stable algorithm (HMAC-SHA256 over a canonical request), and hand-rolling it means this
transport needs nothing more than an HTTP client to work — which is why it runs
unmodified on native Rust, Node.js (via `reqwest`), and WASM/browser targets (`reqwest`
uses `fetch` under `wasm32`). See the [runtime support matrix](/docs/runtime-matrix/).

## Configuration

`SesConfig::new(region, access_key_id, secret_access_key)` is the required baseline;
everything else is an optional builder method:

| Field                 | Type             | Default                                     | Description |
| --------------------- | ---------------- | -------------------------------------------- | ----------- |
| `region`              | `String`         | *(required)*                                 | AWS region, e.g. `"us-east-1"`. Also determines the API host: `email.<region>.amazonaws.com`. |
| `access_key_id`       | `String`         | *(required)*                                 | AWS access key ID. |
| `secret_access_key`   | `String`         | *(required)*                                 | AWS secret access key. |
| `session_token`       | `Option<String>` | `None`                                        | Set via `.with_session_token(token)` for temporary STS credentials; sent as `x-amz-security-token` and folded into the SigV4 signature. |
| `endpoint_override`   | `Option<String>` | `None`                                        | Set via `.with_endpoint_override(url)`. Overrides the scheme+host the request is POSTed to — for a VPC PrivateLink endpoint, or to point tests at a mock server. The `Host` header used for *signing* is unaffected — it's still derived from `region`, so the override is purely about where the bytes are physically sent. |
| `message_id_hostname` | `Option<String>` | sending domain, falling back to the SES host  | Set via `.with_message_id_hostname(hostname)`. See below. |

`SesTransport::new(config)` builds one `reqwest::Client` and reuses it for every send.

### Message-ID hostname resolution

When a `Message` doesn't already carry a `message_id`, the SES transport needs a
hostname to generate one from (the part after the `@` in `<random@hostname>`). It picks,
in order:

1. `message_id_hostname`, if you set one with `.with_message_id_hostname(...)`.
2. Otherwise, the domain of the message's `From` address.
3. Otherwise (no `From` domain available), the SES API endpoint itself
   (`email.<region>.amazonaws.com`).

That third option is what this crate used to do *unconditionally*, which produced
Message-IDs like `<...@email.us-east-1.amazonaws.com>` that don't align with your
actual sending domain — a real alignment problem for anything checking Message-ID
against your DKIM/SPF domain. The From-domain default and the explicit override were
both added to fix that; prefer setting `.with_message_id_hostname("yourdomain.com")`
explicitly if you want a guaranteed value regardless of what a given message's `From`
happens to be.

## How it sends

1. Builds the message to raw RFC 5322 bytes and base64-encodes it into the SES v2
   `SendEmail` request body as `Content.Raw.Data`, with `Destination.ToAddresses` and
   `FromEmailAddress` taken from the message's effective envelope.
2. Signs the request with SigV4 (see below) and POSTs it to
   `https://email.<region>.amazonaws.com/v2/email/outbound-emails` (or your
   `endpoint_override`).
3. On a non-2xx response, returns an error including the response body text (so an
   "email address not verified" or similar SES error reaches your code intact).
4. On success, parses `MessageId` out of the JSON response body and reports it as
   `SentInfo.message_id` — unlike the sendmail/stream/JSON transports, this is always
   the ID AWS actually assigned, not whatever was on the `Message` struct going in.

`SentInfo.raw` is always `None` for this transport (the composed bytes were sent, not
retained), and `rejected` is always empty — the SES v2 `SendEmail` call is all-or-nothing
per this integration, so a partial failure surfaces as an `Err`, not a rejected list.

### The signer

`trustmailer-ses::sigv4` computes the canonical request, string-to-sign, and the
derived signing key chain (`kDate` → `kRegion` → `kService` → `kSigning`) per AWS's
published SigV4 spec, signing exactly `host`, `x-amz-date`, and (when a session token is
present) `x-amz-security-token` — the minimum header set SES's API requires. It's
covered by tests asserting the exact `Authorization` header shape (`AWS4-HMAC-SHA256
Credential=.../aws4_request, SignedHeaders=..., Signature=...`), plus integration tests
against a small hand-rolled HTTP/1.1 mock server (no mocking framework, no real AWS
calls) reached via `.with_endpoint_override(...)`.

:::tip[No AWS SDK in your dependency tree]
Because the signer is self-contained, adding SES support to your binary doesn't pull in
`aws-sdk-sesv2`, `aws-config`, or their transitive dependency graph — just
`trustmailer-ses` and whatever HTTP client backend `reqwest` picks for your target.
:::

## Credentials don't print in plain text

`SesConfig` has a hand-written `Debug` implementation specifically so that logging a
config — `tracing::debug!(?config)`, an assertion failure message, a panic — doesn't
leak secrets. `secret_access_key` and (if set) `session_token` always print as
`***REDACTED***`; `region`, `access_key_id`, `endpoint_override`, and
`message_id_hostname` print normally, since they aren't sensitive on their own.

:::caution[Verified, not assumed]
This is tested directly: constructing a `SesConfig` with a real-looking secret and
session token and formatting it with `{:?}` asserts the secret values never appear in
the output and that `***REDACTED***` does. See [Security](/docs/security/) for the
broader pattern this fits into.
:::

## `verify()`

`SesTransport` doesn't override `Transport::verify()`, so it inherits the default —
`Ok(true)` unconditionally, with no real network call. If you want to confirm your SES
credentials actually work, send a real message (to a verified address, if your account
is still in the SES sandbox) rather than relying on `.verify()`.

## Rust example

```rust
use trustmailer::{Message, Transport};
use trustmailer::ses::{SesConfig, SesTransport};

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let config = SesConfig::new(
        "us-east-1",
        std::env::var("AWS_ACCESS_KEY_ID").unwrap(),
        std::env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
    )
    .with_message_id_hostname("example.com");

    let transport = SesTransport::new(config);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello from SES")
        .text("Hi Bob!")
        .build();

    let info = transport.send(&message).await?;
    println!("SES message id: {}", info.message_id);
    Ok(())
}
```

## Node.js, Bun, and Deno

`createTransport()`'s `ses` option builds an `SesTransport` directly — no SMTP involved,
same HTTPS + hand-rolled SigV4 path as native Rust:

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  ses: {
    region: "us-east-1",
    accessKeyId: process.env.AWS_ACCESS_KEY_ID,
    secretAccessKey: process.env.AWS_SECRET_ACCESS_KEY,
    // sessionToken: "...",            // optional, for temporary/STS credentials
    // endpointOverride: "https://...", // optional, VPC PrivateLink or a mock server
    // messageIdHostname: "example.com", // optional, see "Message-ID hostname" above
  },
});

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello from SES",
  text: "Hi Bob!",
});
console.log("SES message id:", info.messageId);
```

`SesOptions`' fields mirror `SesConfig`'s builder one-to-one (`region`/`accessKeyId`/
`secretAccessKey` required, the rest optional) — verified against
`crates/trustmailer-node/src/lib.rs`'s generated `index.d.ts`, not assumed from the Rust
API. `dkim` composes with it exactly like every other transport option
(`createTransport({ ses: {...}, dkim: {...} })`).

## Browser/WASM availability

`trustmailer-wasm` (browser, Cloudflare Workers, other WASM hosts) also exposes SES, with
a smaller options surface than the Rust crate and the Node binding — `{ kind: "ses",
region, accessKeyId, secretAccessKey }` only, with no `sessionToken`, `endpointOverride`,
or `messageIdHostname` currently wired through. See the [browser/WASM
quickstart](/docs/quickstart/browser-wasm/).

## See also

- [Sendmail transport](/docs/transports/sendmail/) — local-binary delivery instead of
  an HTTPS API.
- [Stream transport](/docs/transports/stream/) and [JSON transport](/docs/transports/json/)
  — render without sending, useful for testing SES-bound messages without real AWS calls.
- [Security](/docs/security/) — the broader audit trail this transport's redacted
  `Debug` impl and SigV4 tests are part of.
- [Runtime support matrix](/docs/runtime-matrix/) — full per-runtime availability.
