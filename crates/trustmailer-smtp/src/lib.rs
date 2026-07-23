//! SMTP transport for trustmailer.
//!
//! Native runtimes only — it opens raw TCP sockets via `tokio`, and most WASM hosts
//! (browsers, generic edge isolates) don't expose that. Two WASM-compatible
//! alternatives, depending on what your host actually allows:
//! - **Cloudflare Workers** genuinely can open outbound TCP (the `cloudflare:sockets`
//!   `connect()` API) — see the `trustmailer-smtp-cfworkers` crate for real SMTP there.
//! - Anywhere else, use `trustmailer-ses` (HTTPS API) or `trustmailer-json`/
//!   `trustmailer-stream` to hand the composed message to your own HTTP relay instead.

mod config;
mod connection;
mod oauth2;
mod pool;
mod proxy;

pub use config::SmtpConfig;
pub use oauth2::{OAuth2Config, OAuth2TokenProvider};
pub use pool::SmtpPool;
pub use proxy::ProxyConfig;
pub use trustmailer_smtp_protocol::Auth;

use async_trait::async_trait;
use connection::SmtpConnection;
use trustmailer_core::{mime, Message, Result, SentInfo, Transport};

pub struct SmtpTransport {
    config: SmtpConfig,
}

impl SmtpTransport {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }

    /// Wraps this configuration in a pooled transport that reuses connections
    /// across sends, up to `config.pool_size` concurrent connections.
    pub fn pooled(self) -> SmtpPool {
        SmtpPool::new(self.config)
    }
}

#[async_trait]
impl Transport for SmtpTransport {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let raw = mime::build(message, &self.config.ehlo_name)?;
        let envelope = message.effective_envelope()?;

        let mut conn = SmtpConnection::connect(&self.config).await?;
        let response = conn
            .send_transaction(&envelope.from, &envelope.to, &raw)
            .await?;
        conn.quit().await?;

        Ok(SentInfo {
            message_id: message.message_id.clone().unwrap_or_default(),
            accepted: envelope.to.clone(),
            rejected: Vec::new(),
            envelope,
            response: Some(response),
            raw: None,
        })
    }

    async fn verify(&self) -> Result<bool> {
        let mut conn = SmtpConnection::connect(&self.config).await?;
        conn.noop().await?;
        conn.quit().await?;
        Ok(true)
    }

    fn name(&self) -> &'static str {
        "smtp"
    }
}
