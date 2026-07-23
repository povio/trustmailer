//! # trustmailer
//!
//! A Rust-powered, [nodemailer](https://nodemailer.com)-compatible mail library —
//! the same message model and transport contract, usable from native Rust, Node.js
//! (`trustmailer-node`, via napi-rs) and the browser/Deno/Bun/Cloudflare Workers
//! (`trustmailer-wasm`, via wasm-bindgen).
//!
//! ```no_run
//! # async fn run() -> trustmailer_core::Result<()> {
//! use trustmailer::{Message, Transport};
//! use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};
//!
//! let config = SmtpConfig::new("smtp.example.com", 587)
//!     .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() });
//! let transport = SmtpTransport::new(config);
//!
//! let message = Message::builder()
//!     .from("Alice <alice@example.com>")
//!     .to("bob@example.com")
//!     .subject("Hello")
//!     .text("Hi Bob!")
//!     .html("<b>Hi Bob!</b>")
//!     .build();
//!
//! let info = transport.send(&message).await?;
//! println!("sent: {}", info.message_id);
//! # Ok(())
//! # }
//! ```

pub use trustmailer_core::*;

// `sendmail` spawns OS processes via `tokio::process::Command`, which has no wasm32
// support. Nothing else in this facade's feature-gating stops
// `cargo build --target wasm32-unknown-unknown -p trustmailer` (default features) from
// being attempted — fail that combination early with a clear message instead of a
// confusing error deep inside tokio's `process` module. WASM/browser/Workers hosts
// should use `trustmailer-wasm` instead.
#[cfg(all(feature = "sendmail", target_arch = "wasm32"))]
compile_error!(
    "trustmailer's `sendmail` feature spawns OS processes and requires a native target; \
     use trustmailer-wasm instead for WASM/browser/Cloudflare Workers hosts."
);

/// DKIM signing (RSA-SHA256, relaxed/relaxed canonicalization) — compose
/// [`dkim::SigningTransport`] around any other transport below. Available on every
/// runtime (native, Node.js, WASM).
#[cfg(feature = "dkim")]
pub use trustmailer_dkim as dkim;
/// Renders a message as JSON instead of sending it (tests, inspection). Available on
/// every runtime.
#[cfg(feature = "json")]
pub use trustmailer_json as json;
/// Pipes the composed message into a local `sendmail`-compatible binary's stdin.
/// Native/Unix-oriented: spawns OS processes, so it requires a native target (see the
/// `compile_error!` guard above — `trustmailer-sendmail` is only a dependency on
/// non-wasm32 targets in the first place) and a `sendmail`-compatible binary on `$PATH`
/// or at an explicit path.
#[cfg(all(feature = "sendmail", not(target_arch = "wasm32")))]
pub use trustmailer_sendmail as sendmail;
/// Amazon SES v2 HTTPS API, hand-rolled SigV4. Available on every runtime (uses
/// `reqwest`, which calls through `fetch` under wasm32).
#[cfg(feature = "ses")]
pub use trustmailer_ses as ses;
/// SMTP over a TCP connection (TLS/STARTTLS, auth, connection pooling, SOCKS/HTTP
/// proxying). Native runtimes only.
#[cfg(feature = "smtp")]
pub use trustmailer_smtp as smtp;
/// Renders the raw composed MIME message instead of sending it (tests, custom relays).
/// Available on every runtime.
#[cfg(feature = "stream")]
pub use trustmailer_stream as stream;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "stream")]
    #[tokio::test]
    async fn end_to_end_stream_send() {
        let message = Message::builder()
            .from("alice@example.com")
            .to("bob@example.com")
            .subject("Hello")
            .text("Hi Bob!")
            .build();
        let transport = stream::StreamTransport::new();
        let info = transport.send(&message).await.unwrap();
        assert!(info.raw.is_some());
    }
}
