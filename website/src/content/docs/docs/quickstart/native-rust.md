---
title: "Quickstart: Native Rust"
description: Send mail from native Rust with trustmailer's SMTP transport.
---

```sh
cargo add trustmailer
cargo build --workspace --exclude trustmailer-node
```

```rust
use trustmailer::{Message, Transport};
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let config = SmtpConfig::new("smtp.example.com", 587)
        .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() });
    let transport = SmtpTransport::new(config);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .html("<b>Hi Bob!</b>")
        .build();

    let info = transport.send(&message).await?;
    println!("sent: {}", info.message_id);
    Ok(())
}
```

`trustmailer` is a facade crate: it re-exports `trustmailer-core` plus every native
transport behind Cargo features (`smtp`, `sendmail`, `ses`, `dkim`, `stream`, `json`),
all enabled by default. Disable the ones you don't need with `default-features = false`.

:::caution[wasm32 target]
The `sendmail` feature spawns OS processes and requires a native target. Building
`trustmailer` for `wasm32-unknown-unknown` with `sendmail` enabled fails on purpose —
use [`trustmailer-wasm`](/docs/quickstart/browser-wasm/) instead for WASM/browser hosts.
:::
