---
title: "Guía rápida: Rust nativo"
description: Envía correo desde Rust nativo con el transporte SMTP de trustmailer.
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

`trustmailer` es un crate fachada: re-exporta `trustmailer-core` más cada transporte
nativo tras features de Cargo (`smtp`, `sendmail`, `ses`, `dkim`, `stream`, `json`), todas
activadas por defecto. Desactiva las que no necesites con `default-features = false`.

:::caution[Target wasm32]
La feature `sendmail` lanza procesos del sistema operativo y requiere un target nativo.
Compilar `trustmailer` para `wasm32-unknown-unknown` con `sendmail` activado falla a
propósito — usa [`trustmailer-wasm`](/es/docs/quickstart/browser-wasm/) para hosts
WASM/navegador.
:::
