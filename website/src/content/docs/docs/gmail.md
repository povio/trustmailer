---
title: Using Gmail
description: Sending through Gmail/Google Workspace with an App Password or OAuth2.
---

Gmail is the single most common target for the SMTP transport in practice, and it's also
the one most likely to reject a naive setup — Google stopped accepting a plain account
password for SMTP login years ago. This page is the practical "make it actually work" path,
built on top of the [SMTP transport reference](/docs/transports/smtp/).

## The well-known shortcut

`SmtpConfig::for_service("gmail")` resolves to the exact values Google publishes for SMTP
submission, verified from trustmailer-core's own `well_known.rs` table:

| | |
| --- | --- |
| Host | `smtp.gmail.com` |
| Port | `465` |
| Secure | `true` (implicit TLS) |

```rust
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};

let config = SmtpConfig::for_service("gmail")
    .expect("gmail is a known service")
    .with_auth(Auth::Plain {
        username: "you@gmail.com".into(),
        password: "your-16-char-app-password".into(),
    });
let transport = SmtpTransport::new(config);
```

`for_service` also recognizes `"googlemail"` (an alias) and, separately,
`"gmailworkspace"`/`"googleworkspacemail"` — that variant points at
`smtp-relay.gmail.com`, Google Workspace's *relay* endpoint (a different product from
consumer Gmail SMTP, intended for domain-authenticated relay use, not a substitute for
picking the auth method below).

## Authenticating: App Password vs. OAuth2

Google's account password does not work for SMTP `AUTH PLAIN`/`LOGIN` — it never has,
for third-party SMTP clients, once 2-Step Verification is enabled (and Google has been
steadily requiring 2SV account-wide). You have two real options:

### App Password (simplest, personal accounts)

Requires 2-Step Verification enabled on the Google account first. Generate one at
[myaccount.google.com/apppasswords](https://myaccount.google.com/apppasswords) — it's a
16-character, space-formatted string scoped just to this use. Use it verbatim as the
`password` in `Auth::Plain`, as shown above. This is the fastest path for a personal Gmail
account and matches how nodemailer users typically set Gmail up too.

:::caution[Not always available]
Google Workspace admins can disable App Passwords org-wide. If password generation is
greyed out or missing from a Workspace account's security settings, OAuth2 is the only
remaining path — see below.
:::

### OAuth2 (Workspace-friendly, no shared secret)

trustmailer's `Auth::XOAuth2` plus `OAuth2Config`/`OAuth2TokenProvider` (documented in
full on the [SMTP transport page](/docs/transports/smtp/#oauth2-xoauth2)) handles Google's
OAuth2 token refresh flow:

```rust
use trustmailer::smtp::{OAuth2Config, OAuth2TokenProvider, SmtpConfig, SmtpTransport};

let oauth2 = OAuth2TokenProvider::new(OAuth2Config::new(
    "you@gmail.com",
    "....apps.googleusercontent.com", // client_id
    "...",                            // client_secret
    "...",                            // refresh_token
));
// OAuth2Config::new() already defaults access_url to Google's token endpoint —
// override with .with_access_url(...) only for a non-Google provider.

let config = SmtpConfig::for_service("gmail")
    .expect("gmail is a known service")
    .with_auth(oauth2.to_auth().await?);
let transport = SmtpTransport::new(config);
```

Getting a Google OAuth2 `client_id`/`client_secret`/`refresh_token` triple is a Google
Cloud Console setup task (enable the Gmail API, create OAuth credentials, run the
consent flow once to mint a refresh token) — that part is identical to what nodemailer's
own Gmail OAuth2 guide walks through, and out of scope for what trustmailer's code
controls.

## Node.js

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  host: "smtp.gmail.com",
  port: 465,
  secure: true,
  auth: { user: "you@gmail.com", pass: "your-16-char-app-password" },
});

const info = await transporter.sendMail({
  from: "you@gmail.com",
  to: "bob@example.com",
  subject: "Hello from Gmail",
  text: "Sent via trustmailer.",
});
```

There's no `for_service`/well-known-lookup shortcut exposed to the Node.js binding today —
pass `smtp.gmail.com`/`465`/`true` directly, as above.

## Common failure

An account password (not an App Password, not an OAuth2 token) in `auth.pass` fails
authentication — trustmailer surfaces this as
[`Error::Transport`](/docs/errors/#transportstring) with the server's rejection reason in
the message, the same failure class nodemailer calls `EAUTH`. If you see an auth failure
against `smtp.gmail.com`, the password field is almost always the thing to check first.
