//! napi-rs bindings exposing a nodemailer-shaped `createTransport()` / `sendMail()`
//! API backed by trustmailer's native Rust transports.

#![deny(clippy::all)]

use base64::{engine::general_purpose::STANDARD, Engine as _};
use napi::bindgen_prelude::*;
use napi::Status;
use napi_derive::napi;
use std::sync::Arc;
use trustmailer_core::Message;

#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Maps every `trustmailer_core::Error` variant to a `napi::Error` whose message is
/// prefixed with a stable `[VariantName]` tag (and whose `Status` is chosen per
/// variant, where a reasonable mapping exists), so JS callers can pattern-match on
/// error kind (e.g. `err.message.startsWith("[NoRecipients]")`) instead of having to
/// substring-match an opaque, unstable message.
fn to_napi_err(e: trustmailer_core::Error) -> napi::Error {
    use trustmailer_core::Error;
    let (status, code) = match &e {
        Error::InvalidAddress(_) => (Status::InvalidArg, "InvalidAddress"),
        Error::NoRecipients => (Status::InvalidArg, "NoRecipients"),
        Error::NoSender => (Status::InvalidArg, "NoSender"),
        Error::Attachment(_) => (Status::InvalidArg, "Attachment"),
        Error::Mime(_) => (Status::GenericFailure, "Mime"),
        Error::Transport(_) => (Status::GenericFailure, "Transport"),
        Error::Io(_) => (Status::GenericFailure, "Io"),
        Error::Dkim(_) => (Status::InvalidArg, "Dkim"),
    };
    napi::Error::new(status, format!("[{code}] {e}"))
}

#[napi(object)]
pub struct SmtpAuthOptions {
    pub user: String,
    pub pass: String,
}

#[napi(object)]
pub struct DkimOptions {
    pub domain_name: String,
    pub key_selector: String,
    pub private_key: String,
}

#[napi(object)]
pub struct SesOptions {
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    /// Overrides the base URL the `SendEmail` request is POSTed to (scheme + host, no
    /// path) — for a VPC PrivateLink endpoint or pointing tests at a mock server.
    pub endpoint_override: Option<String>,
    /// Hostname used only when generating a `Message-ID` for a message that doesn't
    /// already have one. Defaults to the sending domain from the message's `From`
    /// address if unset.
    pub message_id_hostname: Option<String>,
}

#[napi(object)]
pub struct TransportOptions {
    /// Signs every outgoing message with DKIM before handing it to the underlying transport.
    pub dkim: Option<DkimOptions>,
    /// Send over Amazon SES's HTTPS API instead of SMTP.
    pub ses: Option<SesOptions>,
    /// Well-known service name, e.g. `"gmail"` — looks up host/port/secure automatically.
    pub service: Option<String>,
    pub host: Option<String>,
    pub port: Option<u32>,
    pub secure: Option<bool>,
    pub auth: Option<SmtpAuthOptions>,
    /// Use the local `sendmail` binary instead of SMTP.
    pub sendmail: Option<bool>,
    /// Path to the sendmail binary; defaults to `/usr/sbin/sendmail`. This is trusted,
    /// operator-supplied config, not attacker-controllable input — it should generally
    /// be an absolute path, since a relative value falls back to a `PATH` lookup.
    pub path: Option<String>,
    /// Render messages instead of sending them (status in `response`, MIME in `rawBase64`).
    pub stream_transport: Option<bool>,
    /// Render messages as JSON instead of sending them (returns JSON as `response`).
    pub json_transport: Option<bool>,
}

/// Mirrors nodemailer's `attachments: [{ filename, content, contentType, cid }]` shape.
#[napi(object)]
pub struct AttachmentInput {
    pub filename: Option<String>,
    /// Attachment bytes, given either as a `Buffer` or as a base64-encoded string
    /// (equivalent to nodemailer's `content` + `encoding: "base64"`).
    pub content: Either<Buffer, String>,
    pub content_type: Option<String>,
    /// Content-ID for referencing this attachment as an embedded image via `cid:` in
    /// HTML. When set, the attachment is sent with an `inline` disposition.
    pub cid: Option<String>,
}

#[napi(object)]
pub struct MailOptions {
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub reply_to: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    /// Explicit `Message-ID` header value. When omitted, one is auto-generated before
    /// sending and reported back on `SentInfoJs.message_id`, mirroring nodemailer's
    /// `info.messageId` behavior.
    pub message_id: Option<String>,
    pub attachments: Option<Vec<AttachmentInput>>,
    /// Custom headers. Names must be printable ASCII tokens without colons.
    pub headers: Option<std::collections::HashMap<String, String>>,
}

#[napi(object)]
pub struct SentInfoJs {
    pub message_id: String,
    pub accepted: Vec<String>,
    pub rejected: Vec<String>,
    pub response: Option<String>,
    /// Exact MIME bytes encoded as base64, when supplied by the transport.
    pub raw_base64: Option<String>,
}

fn build_attachment(a: AttachmentInput) -> trustmailer_core::Attachment {
    let content = match a.content {
        Either::A(buf) => trustmailer_core::AttachmentContent::Bytes(buf.to_vec()),
        Either::B(base64_str) => trustmailer_core::AttachmentContent::Base64(base64_str),
    };
    let content_disposition = if a.cid.is_some() {
        trustmailer_core::ContentDisposition::Inline
    } else {
        trustmailer_core::ContentDisposition::Attachment
    };
    trustmailer_core::Attachment {
        filename: a.filename,
        content,
        content_type: a.content_type,
        content_disposition,
        cid: a.cid,
        headers: Vec::new(),
    }
}

fn build_message(mail: MailOptions) -> Result<Message> {
    let mut builder = Message::builder();
    if let Some(from) = mail.from {
        builder = builder.from(from.as_str());
    }
    if let Some(to) = mail.to {
        builder = builder.to(to.as_str());
    }
    if let Some(cc) = mail.cc {
        builder = builder.cc(cc.as_str());
    }
    if let Some(bcc) = mail.bcc {
        builder = builder.bcc(bcc.as_str());
    }
    if let Some(reply_to) = mail.reply_to {
        builder = builder.reply_to(reply_to.as_str());
    }
    if let Some(subject) = mail.subject {
        builder = builder.subject(subject);
    }
    if let Some(text) = mail.text {
        builder = builder.text(text);
    }
    if let Some(html) = mail.html {
        builder = builder.html(html);
    }
    if let Some(message_id) = mail.message_id {
        builder = builder.message_id(message_id);
    }
    if let Some(attachments) = mail.attachments {
        for attachment in attachments {
            builder = builder.attachment(build_attachment(attachment));
        }
    }
    if let Some(headers) = mail.headers {
        for (name, value) in headers {
            if name.is_empty() || !name.bytes().all(|b| (33..=126).contains(&b) && b != b':') {
                return Err(napi::Error::new(
                    Status::InvalidArg,
                    "invalid custom header name",
                ));
            }
            builder = builder.header(name, value);
        }
    }
    Ok(builder.build())
}

fn to_sent_info(info: trustmailer_core::SentInfo) -> SentInfoJs {
    SentInfoJs {
        message_id: info.message_id,
        accepted: info.accepted,
        rejected: info.rejected,
        response: info.response,
        raw_base64: info.raw.map(|raw| STANDARD.encode(raw)),
    }
}

/// Render a draft as RFC 5322 MIME encoded in base64, without delivery or envelope
/// validation. Native MIME temporaries are freed before the JS string is returned.
#[napi]
pub fn render_eml_base64(options: MailOptions) -> Result<String> {
    let message = build_message(options)?;
    let raw = trustmailer_core::mime::build(&message, "localhost").map_err(to_napi_err)?;
    Ok(STANDARD.encode(raw))
}

#[napi]
pub struct Transporter {
    inner: Arc<dyn trustmailer_core::Transport + Send + Sync>,
}

#[napi]
impl Transporter {
    #[napi]
    pub async fn send_mail(&self, mail: MailOptions) -> Result<SentInfoJs> {
        let mut message = build_message(mail)?;
        // Pre-generate the Message-ID before building/sending when the caller didn't
        // supply one, so it's populated on `message.message_id` ahead of time — every
        // transport reports `message.message_id` back on `SentInfo`, so this makes
        // `SentInfoJs.message_id` reflect the real generated ID instead of always being
        // `""`, matching nodemailer's `info.messageId` behavior.
        if message.message_id.is_none() {
            message.message_id = Some(trustmailer_core::mime::encode::generate_message_id(
                "localhost",
            ));
        }
        let info = self.inner.send(&message).await.map_err(to_napi_err)?;
        Ok(to_sent_info(info))
    }

    #[napi]
    pub async fn verify(&self) -> Result<bool> {
        self.inner.verify().await.map_err(to_napi_err)
    }

    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner.name().to_string()
    }
}

/// Wraps `transport` in a DKIM-signing decorator when `dkim` options are set, then
/// erases it to the trait object every `Transporter` variant is stored as.
fn finalize(
    transport: impl trustmailer_core::Transport + Send + Sync + 'static,
    dkim: Option<DkimOptions>,
) -> Result<Arc<dyn trustmailer_core::Transport + Send + Sync>> {
    match dkim {
        None => Ok(Arc::new(transport)),
        Some(opts) => {
            // `from_pkcs8_pem` validates `domain_name`/`key_selector` (rejecting CR/LF/
            // `;`/`"` to prevent DKIM-Signature header injection) in addition to
            // parsing the private key, so its failure isn't always a bad key — route
            // it through `to_napi_err` rather than assuming success and rather than
            // mislabeling every failure as a key problem.
            let signer = trustmailer_dkim::DkimSigner::from_pkcs8_pem(
                opts.domain_name,
                opts.key_selector,
                &opts.private_key,
            )
            .map_err(to_napi_err)?;
            Ok(Arc::new(trustmailer_dkim::SigningTransport::new(
                signer, transport,
            )))
        }
    }
}

#[napi]
pub fn create_transport(options: TransportOptions) -> Result<Transporter> {
    if options.stream_transport.unwrap_or(false) {
        return Ok(Transporter {
            inner: finalize(trustmailer_stream::StreamTransport::new(), options.dkim)?,
        });
    }
    if options.json_transport.unwrap_or(false) {
        return Ok(Transporter {
            inner: finalize(trustmailer_json::JsonTransport::new(), options.dkim)?,
        });
    }
    if options.sendmail.unwrap_or(false) {
        let mut t = trustmailer_sendmail::SendmailTransport::new();
        if let Some(path) = options.path {
            t = t.with_path(path);
        }
        return Ok(Transporter {
            inner: finalize(t, options.dkim)?,
        });
    }
    if let Some(ses) = options.ses {
        let mut config =
            trustmailer_ses::SesConfig::new(ses.region, ses.access_key_id, ses.secret_access_key);
        if let Some(token) = ses.session_token {
            config = config.with_session_token(token);
        }
        if let Some(endpoint) = ses.endpoint_override {
            config = config.with_endpoint_override(endpoint);
        }
        if let Some(hostname) = ses.message_id_hostname {
            config = config.with_message_id_hostname(hostname);
        }
        return Ok(Transporter {
            inner: finalize(trustmailer_ses::SesTransport::new(config), options.dkim)?,
        });
    }

    let mut config = if let Some(service) = &options.service {
        trustmailer_smtp::SmtpConfig::for_service(service)
            .ok_or_else(|| napi::Error::from_reason(format!("unknown service: {service}")))?
    } else {
        let host = options
            .host
            .ok_or_else(|| napi::Error::from_reason("host is required when service is not set"))?;
        let port = match options.port {
            Some(p) => u16::try_from(p).map_err(|_| {
                napi::Error::new(
                    Status::InvalidArg,
                    format!("invalid port {p}: must be between 0 and 65535"),
                )
            })?,
            None => 587,
        };
        trustmailer_smtp::SmtpConfig::new(host, port)
    };
    if let Some(secure) = options.secure {
        config = config.with_secure(secure);
    }
    if let Some(auth) = options.auth {
        config = config.with_auth(trustmailer_smtp::Auth::Plain {
            username: auth.user,
            password: auth.pass,
        });
    }

    Ok(Transporter {
        inner: finalize(trustmailer_smtp::SmtpTransport::new(config), options.dkim)?,
    })
}
