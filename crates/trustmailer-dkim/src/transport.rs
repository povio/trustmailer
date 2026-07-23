use crate::DkimSigner;
use async_trait::async_trait;
use std::sync::Arc;
use trustmailer_core::{mime, Message, Result, SentInfo, Transport};

/// Wraps any [`Transport`] so every message is DKIM-signed before being handed to the
/// inner transport — compose it with SMTP, sendmail, SES, etc:
///
/// ```no_run
/// # use trustmailer_dkim::{DkimSigner, SigningTransport};
/// # use trustmailer_core::Transport;
/// # async fn run(inner: impl Transport, signer: DkimSigner) {
/// let transport = SigningTransport::new(signer, inner);
/// # }
/// ```
pub struct SigningTransport<T> {
    signer: Arc<DkimSigner>,
    inner: T,
    hostname: String,
}

impl<T> SigningTransport<T> {
    pub fn new(signer: DkimSigner, inner: T) -> Self {
        Self {
            signer: Arc::new(signer),
            inner,
            hostname: "localhost".to_string(),
        }
    }

    /// Hostname used when generating a `Message-ID` for messages that don't set one.
    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = hostname.into();
        self
    }

    /// Signs `raw` off the async executor thread. RSA-2048 signing is CPU-bound
    /// (milliseconds, not microseconds) — running it inline inside an `async fn` would
    /// block whatever shared Tokio worker thread happens to poll this future, starving
    /// concurrent sends. Native targets offload it via `spawn_blocking`; wasm32 has no
    /// blocking thread pool (and is single-threaded anyway), so it signs inline there.
    #[cfg(not(target_arch = "wasm32"))]
    async fn sign(&self, raw: Vec<u8>) -> Result<Vec<u8>> {
        let signer = self.signer.clone();
        tokio::task::spawn_blocking(move || signer.sign(&raw))
            .await
            .map_err(|e| {
                trustmailer_core::Error::Dkim(format!("DKIM signing task panicked: {e}"))
            })?
    }

    #[cfg(target_arch = "wasm32")]
    async fn sign(&self, raw: Vec<u8>) -> Result<Vec<u8>> {
        self.signer.sign(&raw)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl<T: Transport + Sync> Transport for SigningTransport<T> {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let raw = mime::build(message, &self.hostname)?;
        let signed = self.sign(raw).await?;

        // `message.clone()` deep-copies every attachment's bytes just to attach the
        // already-signed `raw`. We can't shallow-copy just the envelope-relevant
        // fields here instead (leaving attachments/text/html at their defaults):
        // `mime::build` would short-circuit correctly for any transport that calls
        // it again (SMTP/sendmail/stream all do, and only re-clone the now-cheap
        // `raw` bytes — no MIME re-encoding happens, so there's no double-compute),
        // but `trustmailer-json`'s `JsonTransport` reads `subject`/`text`/`html`/
        // `attachments` directly instead of `raw`, so stripping those fields would
        // silently break that (valid, if unusual) composition. A real fix needs
        // `trustmailer-core::Message`'s owned `Vec<u8>` attachment bytes to become
        // cheap-to-clone (`Arc<[u8]>`/`bytes::Bytes`) — out of scope for this crate.
        let mut signed_message = message.clone();
        signed_message.raw = Some(signed);
        self.inner.send(&signed_message).await
    }

    async fn verify(&self) -> Result<bool> {
        self.inner.verify().await
    }

    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trustmailer_stream::StreamTransport;

    const TEST_KEY_PEM: &str = include_str!("../test-fixtures/test_key.pem");

    #[tokio::test]
    async fn signs_before_delegating() {
        let signer = DkimSigner::from_pkcs8_pem("example.com", "default", TEST_KEY_PEM).unwrap();
        let transport = SigningTransport::new(signer, StreamTransport::new());

        let message = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("hi")
            .text("hello")
            .build();

        let info = transport.send(&message).await.unwrap();
        let raw = String::from_utf8(info.raw.unwrap()).unwrap();
        assert!(raw.starts_with("DKIM-Signature:"));
        assert!(raw.contains("hello"));
    }
}
