//! Stream transport: renders a message to a raw RFC 5322 byte buffer instead of
//! delivering it anywhere. Mirrors nodemailer's `streamTransport`. Works on every
//! runtime since it does no I/O at all.

use async_trait::async_trait;
use trustmailer_core::mime;
use trustmailer_core::{Message, Result, SentInfo, Transport};

pub struct StreamTransport {
    hostname: String,
}

impl StreamTransport {
    pub fn new() -> Self {
        Self {
            hostname: "localhost".to_string(),
        }
    }

    pub fn with_hostname(hostname: impl Into<String>) -> Self {
        Self {
            hostname: hostname.into(),
        }
    }

    pub fn render(&self, message: &Message) -> Result<Vec<u8>> {
        mime::build(message, &self.hostname)
    }
}

impl Default for StreamTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Transport for StreamTransport {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let raw = self.render(message)?;
        let envelope = message.effective_envelope()?;
        Ok(SentInfo {
            message_id: message.message_id.clone().unwrap_or_default(),
            accepted: envelope.to.clone(),
            rejected: Vec::new(),
            envelope,
            response: Some("250 stream transport: message rendered, not sent".to_string()),
            raw: Some(raw),
        })
    }

    fn name(&self) -> &'static str {
        "stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trustmailer_core::Message;

    #[tokio::test]
    async fn renders_message_without_sending() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("hi")
            .text("hello")
            .build();
        let transport = StreamTransport::new();
        let info = transport.send(&msg).await.unwrap();
        let raw = info.raw.unwrap();
        assert!(String::from_utf8(raw).unwrap().contains("hello"));
    }

    // Ported from test/stream-transport/stream-transport-test.js: envelope round-trips
    // correctly and a `raw` override is passed through byte-for-byte, unmodified.

    #[tokio::test]
    async fn envelope_reflects_from_and_to() {
        let msg = Message::builder()
            .from("test@valid.sender")
            .to("test@valid.recipient")
            .text("x")
            .build();
        let transport = StreamTransport::new();
        let info = transport.send(&msg).await.unwrap();
        assert_eq!(info.envelope.from, "test@valid.sender");
        assert_eq!(info.envelope.to, vec!["test@valid.recipient".to_string()]);
    }

    #[tokio::test]
    async fn raw_override_is_passed_through_unmodified() {
        let raw_input = b"teretere\r\nvana kere\r\n".repeat(5);
        let mut msg = Message::builder()
            .from("test@valid.sender")
            .to("test@valid.recipient")
            .build();
        msg.raw = Some(raw_input.clone());

        let transport = StreamTransport::new();
        let info = transport.send(&msg).await.unwrap();
        assert_eq!(info.raw.unwrap(), raw_input);
    }

    #[test]
    fn exposes_transport_name() {
        assert_eq!(StreamTransport::new().name(), "stream");
    }
}
