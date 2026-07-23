use crate::envelope::Envelope;
use crate::error::Result;
use crate::message::Message;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Result of a successful send, mirroring nodemailer's `SentMessageInfo`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SentInfo {
    pub message_id: String,
    pub envelope: Envelope,
    pub accepted: Vec<String>,
    pub rejected: Vec<String>,
    /// Transport-specific response text (e.g. the final SMTP server reply).
    pub response: Option<String>,
    /// The raw rendered MIME message, populated by transports that don't actually
    /// deliver anywhere (stream, JSON) so callers/tests can inspect it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Vec<u8>>,
}

/// Common interface every transport (SMTP, sendmail, SES, stream, JSON, ...) implements.
/// Mirrors nodemailer's transport plugin contract (`transport.send(mail, callback)`).
///
/// No `Send` supertrait bound: on `wasm32` (browser/Workers), `reqwest`'s futures wrap
/// JS promises that aren't `Send`, and the target is single-threaded anyway. Native
/// consumers that need `Send + Sync` trait objects (e.g. to share a transport across
/// tokio tasks) should write `dyn Transport + Send + Sync` at the call site instead.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait Transport {
    async fn send(&self, message: &Message) -> Result<SentInfo>;

    /// Verifies the transport is able to send (e.g. SMTP `NOOP`/login check).
    /// Transports for which this isn't meaningful (stream, JSON) should return `Ok(true)`.
    async fn verify(&self) -> Result<bool> {
        Ok(true)
    }

    fn name(&self) -> &'static str;
}
