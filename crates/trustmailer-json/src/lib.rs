//! JSON transport: serializes a message to a JSON object instead of delivering it.
//! Mirrors nodemailer's `jsonTransport`. Useful for tests and for runtimes (browser,
//! edge workers) where you want to hand the composed message off to something else
//! (e.g. an HTTP relay) rather than send it directly.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use trustmailer_core::{Address, Message, Result, SentInfo, Transport};

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonMessage {
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<JsonAttachment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonAttachment {
    pub filename: Option<String>,
    pub content_type: String,
}

impl From<&Message> for JsonMessage {
    fn from(m: &Message) -> Self {
        Self {
            from: m.from.clone(),
            to: m.to.clone(),
            cc: m.cc.clone(),
            bcc: m.bcc.clone(),
            subject: m.subject.clone(),
            text: m.text.clone(),
            html: m.html.clone(),
            attachments: m
                .attachments
                .iter()
                .map(|a| JsonAttachment {
                    filename: a.filename.clone(),
                    content_type: a.resolved_content_type(),
                })
                .collect(),
        }
    }
}

pub struct JsonTransport;

impl JsonTransport {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, message: &Message) -> Result<String> {
        let json_message = JsonMessage::from(message);
        serde_json::to_string(&json_message)
            .map_err(|e| trustmailer_core::Error::Mime(e.to_string()))
    }
}

impl Default for JsonTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Transport for JsonTransport {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let rendered = self.render(message)?;
        let envelope = message.effective_envelope()?;
        Ok(SentInfo {
            message_id: message.message_id.clone().unwrap_or_default(),
            accepted: envelope.to.clone(),
            rejected: Vec::new(),
            envelope,
            response: Some("250 json transport: message rendered, not sent".to_string()),
            raw: Some(rendered.into_bytes()),
        })
    }

    fn name(&self) -> &'static str {
        "json"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trustmailer_core::Message;

    #[tokio::test]
    async fn renders_message_as_json() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("hi")
            .text("hello")
            .build();
        let transport = JsonTransport::new();
        let info = transport.send(&msg).await.unwrap();
        let raw = String::from_utf8(info.raw.unwrap()).unwrap();
        assert!(raw.contains("\"subject\":\"hi\""));
    }

    // Behavioral ports of test/json-transport/json-transport-test.js: our JSON schema
    // is intentionally simpler than nodemailer's (no inline attachment bytes/headers),
    // so these check structure and address parsing, not a byte-exact JSON diff.

    #[tokio::test]
    async fn addresses_are_parsed_into_name_and_address() {
        let msg = Message::builder()
            .from("Andris Reinman <andris.reinman@gmail.com>")
            .to("Andris Kreata <andris@kreata.ee>, andris@nodemailer.com")
            .cc("info@nodemailer.com")
            .subject("Awesome!")
            .build();
        let transport = JsonTransport::new();
        let json = transport.render(&msg).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["from"]["name"], "Andris Reinman");
        assert_eq!(value["from"]["address"], "andris.reinman@gmail.com");
        assert_eq!(value["to"][0]["name"], "Andris Kreata");
        assert_eq!(value["to"][1]["address"], "andris@nodemailer.com");
        assert_eq!(value["to"][1]["name"], "");
        assert_eq!(value["cc"][0]["address"], "info@nodemailer.com");
    }

    #[tokio::test]
    async fn attachments_are_listed_with_filename_and_content_type() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .attachment(
                trustmailer_core::Attachment::from_bytes("img.png", vec![1, 2, 3])
                    .with_content_type("image/png"),
            )
            .build();
        let transport = JsonTransport::new();
        let json = transport.render(&msg).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["attachments"][0]["filename"], "img.png");
        assert_eq!(value["attachments"][0]["content_type"], "image/png");
    }

    #[tokio::test]
    async fn does_not_actually_deliver_anywhere() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .text("hi")
            .build();
        let transport = JsonTransport::new();
        assert_eq!(transport.name(), "json");
        let info = transport.send(&msg).await.unwrap();
        assert!(info.response.unwrap().contains("not sent"));
    }
}
