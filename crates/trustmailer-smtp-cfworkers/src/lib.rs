//! Real SMTP transport for trustmailer running **inside a Cloudflare Worker**, over
//! the `cloudflare:sockets` `connect()` API — genuine outbound TCP with the platform
//! handling TLS (`secureTransport: "on"` for implicit TLS, `"starttls"` + `startTls()`
//! for STARTTLS), not an HTTP-to-SMTP relay.
//!
//! This is *not* a general browser/WASM capability: `cloudflare:sockets` is a workerd
//! built-in module that only resolves inside the actual Cloudflare Workers runtime.
//! Outside of it (a browser tab, `wasm-pack test`, Deno/Bun running this same `.wasm`
//! file directly) `connect()` is simply undefined — use `trustmailer-ses` or the
//! stream/JSON transports there instead. Also note Cloudflare blocks outbound port 25
//! (MX-to-MX relay); SMTP *submission* on 587/465 — what this transport and real-world
//! nodemailer-style usage actually mean by "send mail via SMTP" — is not blocked.
//!
//! Protocol logic (command formatting, response parsing, dot-stuffing, AUTH encoding)
//! is shared with the native `trustmailer-smtp` crate via `trustmailer-smtp-protocol`;
//! only the byte-stream I/O (`socket.rs`, Web Streams instead of `tokio`) differs.

#[cfg(target_arch = "wasm32")]
mod js;
#[cfg(target_arch = "wasm32")]
mod socket;

#[cfg(target_arch = "wasm32")]
use async_trait::async_trait;
#[cfg(target_arch = "wasm32")]
use socket::{CfSocket, SecureTransport};
#[cfg(target_arch = "wasm32")]
use trustmailer_core::{mime, Message, SentInfo, Transport};
#[cfg(target_arch = "wasm32")]
use trustmailer_core::{Error, Result};
pub use trustmailer_smtp_protocol::Auth;
#[cfg(target_arch = "wasm32")]
use trustmailer_smtp_protocol::{
    auth_command, dot_stuff, ehlo_command, mail_from_command, parse_ehlo_capabilities,
    rcpt_to_command, validate_envelope_address, ResponseParser,
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue};

#[cfg(target_arch = "wasm32")]
fn js_err(context: &str, e: JsValue) -> Error {
    // Prefer a real `Error`/`DOMException`-shaped message (`.message()`), then fall back
    // to a plain string throw, and only fall back to a generic debug format as a last
    // resort — most real socket-level failures surface as thrown `Error` objects, not
    // bare strings, and the previous `as_string()`-only check silently lost that text.
    let msg = e
        .dyn_ref::<js_sys::Error>()
        .map(|err| err.message().as_string().unwrap_or_default())
        .filter(|s| !s.is_empty())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"));
    Error::Transport(format!("{context}: {msg}"))
}

#[derive(Debug, Clone)]
pub struct CfSmtpConfig {
    pub host: String,
    pub port: u16,
    /// Implicit TLS (port 465 style). When `false`, STARTTLS is negotiated (port 587 style)
    /// unless `require_tls` is also turned off.
    pub secure: bool,
    /// Whether a `!secure` connection must upgrade via STARTTLS. Only meant to be turned
    /// off against a trusted plaintext-only server (e.g. local integration tests) —
    /// real-world SMTP submission should always end up encrypted one way or the other.
    pub require_tls: bool,
    pub auth: Option<Auth>,
    pub ehlo_name: String,
}

impl CfSmtpConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            secure: port == 465,
            require_tls: true,
            auth: None,
            ehlo_name: "localhost".to_string(),
        }
    }

    pub fn for_service(service: &str) -> Option<Self> {
        trustmailer_core::well_known::lookup_service(service)
            .map(|s| Self::new(s.host, s.port).with_secure(s.secure))
    }

    pub fn with_secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    pub fn with_auth(mut self, auth: Auth) -> Self {
        self.auth = Some(auth);
        self
    }

    pub fn with_ehlo_name(mut self, name: impl Into<String>) -> Self {
        self.ehlo_name = trustmailer_smtp_protocol::strip_crlf(&name.into());
        self
    }

    pub fn with_require_tls(mut self, require_tls: bool) -> Self {
        self.require_tls = require_tls;
        self
    }
}

/// SMTP transport for use inside a Cloudflare Worker. See the crate-level docs for
/// where this does and doesn't work.
#[cfg(target_arch = "wasm32")]
pub struct CfSmtpTransport {
    config: CfSmtpConfig,
}

#[cfg(target_arch = "wasm32")]
impl CfSmtpTransport {
    pub fn new(config: CfSmtpConfig) -> Self {
        Self { config }
    }

    #[cfg(target_arch = "wasm32")]
    async fn connect(&self) -> Result<CfSocket> {
        let secure_transport = match (self.config.secure, self.config.require_tls) {
            (true, _) => SecureTransport::On,
            (false, true) => SecureTransport::StartTls,
            (false, false) => SecureTransport::Off,
        };
        let mut conn = CfSocket::connect(&self.config.host, self.config.port, secure_transport)
            .await
            .map_err(|e| {
                js_err(
                    &format!(
                        "connect to {}:{} failed",
                        self.config.host, self.config.port
                    ),
                    e,
                )
            })?;

        // Any failure past this point means the underlying `cloudflare:sockets` socket
        // is genuinely open and must be closed before we propagate the error — otherwise
        // a rejected STARTTLS/AUTH/etc leaks the socket for the lifetime of the Worker
        // isolate (see `close_after` below for the same pattern used in send()/verify()).
        match self.handshake(&mut conn).await {
            Ok(()) => Ok(conn),
            Err(e) => {
                let _ = conn.write_line("QUIT").await;
                let _ = conn.close().await;
                Err(e)
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    async fn handshake(&self, conn: &mut CfSocket) -> Result<()> {
        read_greeting(conn).await?;
        let mut caps = ehlo(conn, &self.config.ehlo_name).await?;

        if !self.config.secure && self.config.require_tls {
            if !caps.iter().any(|c| c.eq_ignore_ascii_case("STARTTLS")) {
                return Err(Error::Transport(
                    "server does not advertise STARTTLS".into(),
                ));
            }
            conn.write_line("STARTTLS")
                .await
                .map_err(|e| js_err("STARTTLS failed", e))?;
            expect_code(conn, 220).await?;
            conn.start_tls()
                .await
                .map_err(|e| js_err("TLS upgrade failed", e))?;
            caps = ehlo(conn, &self.config.ehlo_name).await?;
        }

        if let Some(auth) = &self.config.auth {
            if !caps
                .iter()
                .any(|c| c.to_ascii_uppercase().starts_with("AUTH"))
            {
                return Err(Error::Transport("server does not advertise AUTH".into()));
            }
            authenticate(conn, auth).await?;
        }

        Ok(())
    }
}

/// Best-effort graceful shutdown, run *regardless* of whether the preceding SMTP
/// transaction (`result`) succeeded or failed, then the original `result` is returned
/// unchanged. This is the actual fix for the socket leak: previously `QUIT`/`close()`
/// only ran on the success path, so any early `?`-propagated error out of
/// `send_transaction`/the NOOP check skipped cleanup entirely and leaked the socket.
#[cfg(target_arch = "wasm32")]
async fn close_after<T>(conn: &mut CfSocket, result: Result<T>) -> Result<T> {
    let _ = conn.write_line("QUIT").await;
    let _ = conn.close().await;
    result
}

// This crate only ever runs on wasm32 (inside workerd) — `cloudflare:sockets` doesn't
// exist anywhere else. The `Transport` impl is gated to `wasm32` specifically (rather
// than provided unconditionally like the other transports) because it needs the `?Send`
// variant: `wasm-bindgen`'s `JsFuture` is `!Send`, even in its native compile-time stub,
// so a native build of this impl can't satisfy the trait's `Send` bound. Gating it out
// entirely on native lets `cargo test`/`cargo check --workspace` still build this crate's
// plain config/builder tests without needing `--target wasm32-unknown-unknown`.
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl Transport for CfSmtpTransport {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let raw = mime::build(message, &self.config.ehlo_name)?;
        let envelope = message.effective_envelope()?;

        let mut conn = self.connect().await?;
        let result = send_transaction(&mut conn, &envelope.from, &envelope.to, &raw).await;
        let response = close_after(&mut conn, result).await?;

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
        let mut conn = self.connect().await?;
        let result = verify_noop(&mut conn).await;
        close_after(&mut conn, result).await?;
        Ok(true)
    }

    fn name(&self) -> &'static str {
        "smtp-cfworkers"
    }
}

#[cfg(target_arch = "wasm32")]
async fn verify_noop(conn: &mut CfSocket) -> Result<()> {
    conn.write_line("NOOP")
        .await
        .map_err(|e| js_err("NOOP failed", e))?;
    expect_code(conn, 250).await?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn authenticate(conn: &mut CfSocket, auth: &Auth) -> Result<()> {
    match auth {
        Auth::Plain { .. } | Auth::XOAuth2 { .. } => {
            conn.write_line(&auth_command(auth))
                .await
                .map_err(|e| js_err("AUTH failed", e))?;
            expect_code(conn, 235).await?;
        }
        Auth::Login { username, password } => {
            conn.write_line("AUTH LOGIN")
                .await
                .map_err(|e| js_err("AUTH LOGIN failed", e))?;
            expect_code(conn, 334).await?;
            conn.write_line(&Auth::encode(username))
                .await
                .map_err(|e| js_err("AUTH LOGIN username failed", e))?;
            expect_code(conn, 334).await?;
            conn.write_line(&Auth::encode(password))
                .await
                .map_err(|e| js_err("AUTH LOGIN password failed", e))?;
            expect_code(conn, 235).await?;
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn send_transaction(
    conn: &mut CfSocket,
    from: &str,
    to: &[String],
    data: &[u8],
) -> Result<String> {
    validate_envelope_address(from, "sender")?;
    conn.write_line(&mail_from_command(from))
        .await
        .map_err(|e| js_err("MAIL FROM failed", e))?;
    expect_code(conn, 250).await?;

    for rcpt in to {
        validate_envelope_address(rcpt, "recipient")?;
        conn.write_line(&rcpt_to_command(rcpt))
            .await
            .map_err(|e| js_err("RCPT TO failed", e))?;
        expect_code(conn, 250).await?;
    }

    conn.write_line("DATA")
        .await
        .map_err(|e| js_err("DATA failed", e))?;
    expect_code(conn, 354).await?;

    // Known follow-up (documented, not fixed here): `dot_stuff` materializes the whole
    // stuffed message in memory (2-3x size for base64 attachments) rather than streaming
    // line-by-line. Doing this properly means adding a streaming/chunked dot-stuffing
    // API to the shared `trustmailer-smtp-protocol` crate (used by the native
    // `trustmailer-smtp` transport too) and changing `write_raw`'s call sites across both
    // to feed it incrementally — out of scope for a `write_raw`-signature-preserving fix.
    conn.write_raw(&dot_stuff(data))
        .await
        .map_err(|e| js_err("writing message body failed", e))?;
    conn.write_raw(b"\r\n.\r\n")
        .await
        .map_err(|e| js_err("writing end-of-data marker failed", e))?;

    let (code, text) = read_response(conn).await?;
    if code != 250 {
        return Err(Error::Transport(format!(
            "server rejected message: {code} {text}"
        )));
    }
    Ok(text)
}

#[cfg(target_arch = "wasm32")]
async fn read_greeting(conn: &mut CfSocket) -> Result<()> {
    let (code, _) = read_response(conn).await?;
    if code != 220 {
        return Err(Error::Transport(format!(
            "unexpected greeting code: {code}"
        )));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn ehlo(conn: &mut CfSocket, ehlo_name: &str) -> Result<Vec<String>> {
    conn.write_line(&ehlo_command(ehlo_name))
        .await
        .map_err(|e| js_err("EHLO failed", e))?;
    let (code, text) = read_response(conn).await?;
    if code != 250 {
        return Err(Error::Transport(format!("EHLO rejected: {code} {text}")));
    }
    Ok(parse_ehlo_capabilities(&text))
}

#[cfg(target_arch = "wasm32")]
async fn expect_code(conn: &mut CfSocket, expected: u16) -> Result<String> {
    let (code, text) = read_response(conn).await?;
    if code != expected {
        return Err(Error::Transport(format!(
            "expected SMTP code {expected}, got {code}: {text}"
        )));
    }
    Ok(text)
}

#[cfg(target_arch = "wasm32")]
async fn read_response(conn: &mut CfSocket) -> Result<(u16, String)> {
    let mut parser = ResponseParser::new();
    loop {
        let line = conn
            .read_line()
            .await
            .map_err(|e| js_err("reading server response failed", e))?;
        if let Some(result) = parser.feed_line(&line)? {
            return Ok(result);
        }
    }
}

// Pure config/builder tests — no `cloudflare:sockets` interop involved, so these run
// under plain `cargo test` on a native host. The actual protocol path is verified by a
// real `wrangler dev` + `workerd` run against a local fake SMTP server (not something
// `cargo test` can drive); see the crate README for that reproduction.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_secure_by_port_465_and_require_tls_true() {
        let cfg = CfSmtpConfig::new("smtp.example.com", 465);
        assert!(cfg.secure);
        assert!(cfg.require_tls);

        let cfg = CfSmtpConfig::new("smtp.example.com", 587);
        assert!(!cfg.secure);
        assert!(cfg.require_tls);
    }

    #[test]
    fn with_require_tls_false_allows_plaintext_testing() {
        let cfg = CfSmtpConfig::new("127.0.0.1", 2525).with_require_tls(false);
        assert!(!cfg.secure);
        assert!(!cfg.require_tls);
    }

    #[test]
    fn ehlo_name_strips_crlf() {
        let cfg = CfSmtpConfig::new("h", 587).with_ehlo_name("a\r\nMAIL FROM:<x>");
        assert!(!cfg.ehlo_name.contains('\r'));
        assert!(!cfg.ehlo_name.contains('\n'));
    }

    #[test]
    fn for_service_resolves_well_known_provider() {
        let cfg = CfSmtpConfig::for_service("gmail").unwrap();
        assert_eq!(cfg.host, "smtp.gmail.com");
        assert!(cfg.secure);
    }
}
