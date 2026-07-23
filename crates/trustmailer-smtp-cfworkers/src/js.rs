//! `#[wasm_bindgen]` JS-facing surface, matching `trustmailer-wasm`'s
//! `createTransport()`/`sendMail()` shape so this can be merged into that crate's
//! `kind` options once validated. Kept separate for now since this is the one
//! transport that only works inside an actual Cloudflare Worker.

use crate::{Auth, CfSmtpConfig, CfSmtpTransport};
use serde::{Deserialize, Serialize};
use trustmailer_core::{Message, Transport as _};
use wasm_bindgen::prelude::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MailInput {
    from: Option<String>,
    to: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    #[serde(default, rename = "replyTo")]
    reply_to: Option<String>,
    subject: Option<String>,
    text: Option<String>,
    html: Option<String>,
}

fn build_message(mail: MailInput) -> Message {
    let mut builder = Message::builder();
    if let Some(v) = mail.from {
        builder = builder.from(v.as_str());
    }
    if let Some(v) = mail.to {
        builder = builder.to(v.as_str());
    }
    if let Some(v) = mail.cc {
        builder = builder.cc(v.as_str());
    }
    if let Some(v) = mail.bcc {
        builder = builder.bcc(v.as_str());
    }
    if let Some(v) = mail.reply_to {
        builder = builder.reply_to(v.as_str());
    }
    if let Some(v) = mail.subject {
        builder = builder.subject(v);
    }
    if let Some(v) = mail.text {
        builder = builder.text(v);
    }
    if let Some(v) = mail.html {
        builder = builder.html(v);
    }
    builder.build()
}

/// Discriminates which `Auth` variant to build. Defaults to `"plain"` when the JS caller
/// omits `type` entirely, preserving the previous plain-only behavior for existing callers.
#[derive(Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum AuthType {
    #[default]
    Plain,
    Login,
    XOAuth2,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthInput {
    #[serde(default, rename = "type")]
    auth_type: AuthType,
    user: String,
    pass: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransportOptions {
    host: String,
    port: u16,
    #[serde(default)]
    secure: Option<bool>,
    #[serde(default)]
    auth: Option<AuthInput>,
    #[serde(default)]
    name: Option<String>,
    /// Only for testing against a trusted plaintext server — see `CfSmtpConfig::with_require_tls`.
    #[serde(default, rename = "requireTLS")]
    require_tls: Option<bool>,
}

/// Builds a real JS `Error` (not a bare thrown string) so callers get `.stack` and
/// `instanceof Error` working.
fn js_error(msg: impl AsRef<str>) -> JsValue {
    js_sys::Error::new(msg.as_ref()).into()
}

fn to_js_err(e: trustmailer_core::Error) -> JsValue {
    js_error(e.to_string())
}

#[wasm_bindgen]
pub struct CfTransporter {
    inner: CfSmtpTransport,
}

#[wasm_bindgen(js_name = createTransport)]
pub fn create_transport(options: JsValue) -> Result<CfTransporter, JsValue> {
    let options: TransportOptions = serde_wasm_bindgen::from_value(options)
        .map_err(|e| js_error(format!("invalid transport options: {e}")))?;

    let mut config = CfSmtpConfig::new(options.host, options.port);
    if let Some(secure) = options.secure {
        config = config.with_secure(secure);
    }
    if let Some(name) = options.name {
        config = config.with_ehlo_name(name);
    }
    if let Some(auth) = options.auth {
        let mapped = match auth.auth_type {
            AuthType::Plain => Auth::Plain {
                username: auth.user,
                password: auth.pass,
            },
            AuthType::Login => Auth::Login {
                username: auth.user,
                password: auth.pass,
            },
            AuthType::XOAuth2 => Auth::XOAuth2 {
                username: auth.user,
                access_token: auth.pass,
            },
        };
        config = config.with_auth(mapped);
    }
    if let Some(require_tls) = options.require_tls {
        config = config.with_require_tls(require_tls);
    }

    Ok(CfTransporter {
        inner: CfSmtpTransport::new(config),
    })
}

#[wasm_bindgen]
impl CfTransporter {
    #[wasm_bindgen(js_name = sendMail)]
    pub async fn send_mail(&self, mail: JsValue) -> Result<JsValue, JsValue> {
        let mail: MailInput = serde_wasm_bindgen::from_value(mail)
            .map_err(|e| js_error(format!("invalid mail options: {e}")))?;
        let message = build_message(mail);
        let info = self.inner.send(&message).await.map_err(to_js_err)?;

        let out = serde_json::json!({
            "messageId": info.message_id,
            "accepted": info.accepted,
            "rejected": info.rejected,
            "response": info.response,
        });
        // Plain objects, not JS `Map`s — serde-wasm-bindgen's default produces a `Map`,
        // which `Response.json()`/`JSON.stringify()` silently serialize as `{}`.
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        out.serialize(&serializer)
            .map_err(|e| js_error(e.to_string()))
    }

    #[wasm_bindgen]
    pub async fn verify(&self) -> Result<bool, JsValue> {
        self.inner.verify().await.map_err(to_js_err)
    }
}
