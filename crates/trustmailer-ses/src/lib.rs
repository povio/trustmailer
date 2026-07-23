//! Amazon SES transport via the SES v2 `SendEmail` HTTPS API, hand-signed with
//! AWS SigV4 (no `aws-sdk-*` / `aws-config` dependency). Works on every runtime
//! that has an HTTP client — native, Node (via reqwest), and WASM/browser
//! (`reqwest` uses `fetch` under `wasm32`).

mod sigv4;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use sigv4::SigningInput;
use time::macros::format_description;
use trustmailer_core::{mime, Error, Message, Result, SentInfo, Transport};

pub struct SesConfig {
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    /// Overrides the base URL the `SendEmail` request is POSTed to (scheme + host,
    /// no path), for a VPC PrivateLink endpoint or for pointing tests at a mock
    /// server. The `Host` header used for SigV4 signing is unaffected.
    pub endpoint_override: Option<String>,
    /// Hostname used only for generating a `Message-ID` when the message doesn't
    /// already have one (see [`SesConfig::with_message_id_hostname`]).
    message_id_hostname: Option<String>,
}

// Hand-written to redact AWS secrets: the derived impl would print
// `secret_access_key`/`session_token` in plaintext, which is a realistic credential
// leak the moment anyone logs a config with `tracing::debug!(?config)` or similar.
impl std::fmt::Debug for SesConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SesConfig")
            .field("region", &self.region)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"***REDACTED***")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "***REDACTED***"),
            )
            .field("endpoint_override", &self.endpoint_override)
            .field("message_id_hostname", &self.message_id_hostname)
            .finish()
    }
}

impl SesConfig {
    pub fn new(
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self {
            region: region.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
            endpoint_override: None,
            message_id_hostname: None,
        }
    }

    pub fn with_session_token(mut self, token: impl Into<String>) -> Self {
        self.session_token = Some(token.into());
        self
    }

    pub fn with_endpoint_override(mut self, url: impl Into<String>) -> Self {
        self.endpoint_override = Some(url.into());
        self
    }

    /// Hostname used when generating a `Message-ID` for messages that don't already
    /// set one. Defaults (if unset) to the sending domain taken from the message's
    /// `From` address at send time, falling back to the SES API endpoint
    /// (`email.<region>.amazonaws.com`) only if `From` isn't available either — that
    /// fallback is what this crate used unconditionally before, which produced
    /// Message-IDs like `<...@email.us-east-1.amazonaws.com>` that don't align with
    /// the sending domain.
    pub fn with_message_id_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.message_id_hostname = Some(hostname.into());
        self
    }

    fn host(&self) -> String {
        format!("email.{}.amazonaws.com", self.region)
    }
}

pub struct SesTransport {
    config: SesConfig,
    http: reqwest::Client,
}

impl SesTransport {
    pub fn new(config: SesConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct RawContent<'a> {
    #[serde(rename = "Data")]
    data: &'a str,
}

#[derive(Serialize)]
struct Content<'a> {
    #[serde(rename = "Raw")]
    raw: RawContent<'a>,
}

#[derive(Serialize)]
struct SendEmailRequest<'a> {
    #[serde(rename = "Content")]
    content: Content<'a>,
    #[serde(rename = "Destination")]
    destination: Destination,
    #[serde(rename = "FromEmailAddress")]
    from_email_address: &'a str,
}

#[derive(Serialize)]
struct Destination {
    #[serde(rename = "ToAddresses")]
    to_addresses: Vec<String>,
}

/// Hostname to use for a generated `Message-ID` (see [`SesConfig::with_message_id_hostname`]):
/// the configured override if set, otherwise the sending domain taken from the
/// message's `From` address, otherwise the SES API endpoint as a last resort.
fn message_id_hostname(config: &SesConfig, message: &Message) -> String {
    if let Some(hostname) = &config.message_id_hostname {
        return hostname.clone();
    }
    if let Some(domain) = message
        .from
        .as_ref()
        .and_then(|from| from.address.split('@').nth(1))
        .filter(|domain| !domain.is_empty())
    {
        return domain.to_string();
    }
    config.host()
}

fn amz_date_now() -> Result<String> {
    const FORMAT: &[time::format_description::FormatItem] =
        format_description!("[year][month][day]T[hour][minute][second]Z");
    time::OffsetDateTime::now_utc()
        .format(FORMAT)
        .map_err(|e| Error::Transport(format!("failed to format date: {e}")))
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Transport for SesTransport {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let raw = mime::build(message, &message_id_hostname(&self.config, message))?;
        let envelope = message.effective_envelope()?;
        let raw_b64 = STANDARD.encode(&raw);

        let request = SendEmailRequest {
            content: Content {
                raw: RawContent { data: &raw_b64 },
            },
            destination: Destination {
                to_addresses: envelope.to.clone(),
            },
            from_email_address: &envelope.from,
        };
        let body = serde_json::to_vec(&request)
            .map_err(|e| Error::Transport(format!("failed to encode SES request: {e}")))?;

        let host = self.config.host();
        let amz_date = amz_date_now()?;
        let uri = "/v2/email/outbound-emails";

        let authorization = sigv4::sign(&SigningInput {
            method: "POST",
            uri,
            host: &host,
            region: &self.config.region,
            service: "ses",
            amz_date: &amz_date,
            body: &body,
            access_key_id: &self.config.access_key_id,
            secret_access_key: &self.config.secret_access_key,
            session_token: self.config.session_token.as_deref(),
        });

        let url = match &self.config.endpoint_override {
            Some(base) => format!("{}{uri}", base.trim_end_matches('/')),
            None => format!("https://{host}{uri}"),
        };
        let mut req = self
            .http
            .post(&url)
            .header("host", &host)
            .header("x-amz-date", &amz_date)
            .header("content-type", "application/json")
            .header("authorization", authorization)
            .body(body);
        if let Some(token) = &self.config.session_token {
            req = req.header("x-amz-security-token", token);
        }

        let response = req
            .send()
            .await
            .map_err(|e| Error::Transport(format!("SES request failed: {e}")))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Error::Transport(format!(
                "SES rejected the request ({status}): {text}"
            )));
        }

        let message_id = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("MessageId")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();

        Ok(SentInfo {
            message_id,
            accepted: envelope.to.clone(),
            rejected: Vec::new(),
            envelope,
            response: Some(text),
            raw: None,
        })
    }

    fn name(&self) -> &'static str {
        "ses"
    }
}

// Behavioral ports of test/ses-transport/ses-transport-test.js against a small
// hand-rolled HTTP/1.1 mock server (no mocking framework / no real AWS calls), routed
// to via `SesConfig::with_endpoint_override`.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use trustmailer_core::Message;

    /// Starts a one-shot HTTP/1.1 mock server that reads a single request and replies
    /// with `status`/`body`, returning its base URL (`http://127.0.0.1:PORT`).
    async fn mock_server(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = socket.read(&mut buf).await;

            let reason = if status == 200 { "OK" } else { "Bad Request" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });

        format!("http://127.0.0.1:{port}")
    }

    /// Like `mock_server`, but also hands back the raw bytes of the single request it
    /// received, so tests can inspect what was actually sent (e.g. the generated
    /// `Message-ID` hostname buried in the base64-encoded MIME body).
    async fn mock_server_capturing(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::sync::oneshot::Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65536];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let _ = tx.send(buf[..n].to_vec());

            let reason = if status == 200 { "OK" } else { "Bad Request" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });

        (format!("http://127.0.0.1:{port}"), rx)
    }

    /// Decodes the raw RFC 5322 message SES's mock received back out of the
    /// captured HTTP request bytes (`Content.Raw.Data`, base64).
    fn decode_sent_raw_message(request_bytes: &[u8]) -> String {
        let request = String::from_utf8_lossy(request_bytes);
        let body_start = request.find("\r\n\r\n").expect("request must have a body") + 4;
        let json_body: serde_json::Value =
            serde_json::from_str(&request[body_start..]).expect("body must be valid JSON");
        let raw_b64 = json_body["Content"]["Raw"]["Data"]
            .as_str()
            .expect("Content.Raw.Data must be present");
        let raw = STANDARD.decode(raw_b64).expect("Data must be valid base64");
        String::from_utf8_lossy(&raw).into_owned()
    }

    fn test_message() -> Message {
        Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("hi")
            .text("hello")
            .build()
    }

    #[tokio::test]
    async fn returns_the_message_id_from_a_successful_response() {
        let base = mock_server(200, r#"{"MessageId":"010f0197-test-message-id"}"#).await;
        let config =
            SesConfig::new("us-east-1", "AKIDEXAMPLE", "secret").with_endpoint_override(base);
        let transport = SesTransport::new(config);

        let info = transport.send(&test_message()).await.unwrap();
        assert_eq!(info.message_id, "010f0197-test-message-id");
        assert_eq!(info.accepted, vec!["b@example.com".to_string()]);
    }

    #[tokio::test]
    async fn message_id_defaults_to_the_from_address_domain() {
        let (base, rx) = mock_server_capturing(200, r#"{"MessageId":"id"}"#).await;
        let config =
            SesConfig::new("us-east-1", "AKIDEXAMPLE", "secret").with_endpoint_override(base);
        let transport = SesTransport::new(config);

        transport.send(&test_message()).await.unwrap();
        let raw = decode_sent_raw_message(&rx.await.unwrap());

        assert!(
            raw.contains("@example.com>"),
            "Message-ID should be generated using the From domain, got: {raw}"
        );
        assert!(!raw.contains("amazonaws.com"));
    }

    #[tokio::test]
    async fn message_id_hostname_override_takes_priority() {
        let (base, rx) = mock_server_capturing(200, r#"{"MessageId":"id"}"#).await;
        let config = SesConfig::new("us-east-1", "AKIDEXAMPLE", "secret")
            .with_endpoint_override(base)
            .with_message_id_hostname("mail.other-domain.test");
        let transport = SesTransport::new(config);

        transport.send(&test_message()).await.unwrap();
        let raw = decode_sent_raw_message(&rx.await.unwrap());

        assert!(raw.contains("@mail.other-domain.test>"));
    }

    #[tokio::test]
    async fn surfaces_an_error_response_as_a_transport_error() {
        let base = mock_server(400, r#"{"message":"Email address not verified"}"#).await;
        let config =
            SesConfig::new("us-east-1", "AKIDEXAMPLE", "secret").with_endpoint_override(base);
        let transport = SesTransport::new(config);

        let err = transport.send(&test_message()).await.unwrap_err();
        assert!(err.to_string().contains("not verified"));
    }

    #[test]
    fn debug_redacts_secrets() {
        let config = SesConfig::new("us-east-1", "AKIDEXAMPLE", "super-secret-value")
            .with_session_token("super-secret-token");
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret-value"));
        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("***REDACTED***"));
        assert!(debug.contains("AKIDEXAMPLE")); // non-secret fields stay visible
    }

    #[test]
    fn request_is_signed_with_sigv4() {
        // SigV4 is exercised indirectly by the mock-server tests above (a malformed
        // Authorization header would still reach the mock, since it doesn't verify
        // signatures) — this checks the Authorization header shape directly instead.
        let input = sigv4::SigningInput {
            method: "POST",
            uri: "/v2/email/outbound-emails",
            host: "email.us-east-1.amazonaws.com",
            region: "us-east-1",
            service: "ses",
            amz_date: "20240101T000000Z",
            body: b"{}",
            access_key_id: "AKIDEXAMPLE",
            secret_access_key: "secret",
            session_token: None,
        };
        let auth = sigv4::sign(&input);
        assert!(auth.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20240101/us-east-1/ses/aws4_request"
        ));
        assert!(auth.contains("SignedHeaders=host;x-amz-date"));
    }
}
