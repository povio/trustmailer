//! wasm-bindgen bindings for trustmailer: browser, Deno, Bun, Cloudflare Workers.
//!
//! There is no raw-SMTP transport here — WASM sandboxes don't expose TCP sockets —
//! so the choices are the JSON/stream transports (render the message, deliver it
//! yourself via `fetch`/your own relay) or the SES transport (an HTTPS API call,
//! which `reqwest` makes through `fetch` automatically on `wasm32`).

use serde::{Deserialize, Serialize};
use trustmailer_core::{Message, Transport as _};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MailInput {
    from: Option<String>,
    to: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    #[serde(rename = "replyTo")]
    reply_to: Option<String>,
    subject: Option<String>,
    text: Option<String>,
    html: Option<String>,
}

fn build_message(mail: MailInput) -> Result<Message, JsValue> {
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
    Ok(builder.build())
}

/// Builds a real JS `Error` (not a bare thrown string) so callers get `.stack` and
/// `instanceof Error` working.
fn js_error(msg: impl AsRef<str>) -> JsValue {
    js_sys::Error::new(msg.as_ref()).into()
}

fn to_js_err(e: trustmailer_core::Error) -> JsValue {
    js_error(e.to_string())
}

#[derive(serde::Serialize)]
struct SentInfoOut {
    #[serde(rename = "messageId")]
    message_id: String,
    accepted: Vec<String>,
    rejected: Vec<String>,
    response: Option<String>,
}

impl From<trustmailer_core::SentInfo> for SentInfoOut {
    fn from(info: trustmailer_core::SentInfo) -> Self {
        Self {
            message_id: info.message_id,
            accepted: info.accepted,
            rejected: info.rejected,
            response: info
                .response
                .or_else(|| info.raw.map(|r| String::from_utf8_lossy(&r).to_string())),
        }
    }
}

enum Backend {
    Stream(trustmailer_stream::StreamTransport),
    Json(trustmailer_json::JsonTransport),
    Ses(trustmailer_ses::SesTransport),
}

#[wasm_bindgen]
pub struct Transporter {
    backend: Backend,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum TransportOptions {
    Stream,
    Json,
    Ses {
        region: String,
        #[serde(rename = "accessKeyId")]
        access_key_id: String,
        #[serde(rename = "secretAccessKey")]
        secret_access_key: String,
    },
}

#[wasm_bindgen(js_name = createTransport)]
pub fn create_transport(options: JsValue) -> Result<Transporter, JsValue> {
    let options: TransportOptions = serde_wasm_bindgen::from_value(options)
        .map_err(|e| js_error(format!("invalid transport options: {e}")))?;
    let backend = match options {
        TransportOptions::Stream => Backend::Stream(trustmailer_stream::StreamTransport::new()),
        TransportOptions::Json => Backend::Json(trustmailer_json::JsonTransport::new()),
        TransportOptions::Ses {
            region,
            access_key_id,
            secret_access_key,
        } => {
            let config = trustmailer_ses::SesConfig::new(region, access_key_id, secret_access_key);
            Backend::Ses(trustmailer_ses::SesTransport::new(config))
        }
    };
    Ok(Transporter { backend })
}

#[wasm_bindgen]
impl Transporter {
    #[wasm_bindgen(js_name = sendMail)]
    pub async fn send_mail(&self, mail: JsValue) -> Result<JsValue, JsValue> {
        let mail: MailInput = serde_wasm_bindgen::from_value(mail)
            .map_err(|e| js_error(format!("invalid mail options: {e}")))?;
        let message = build_message(mail)?;

        let info = match &self.backend {
            Backend::Stream(t) => t.send(&message).await.map_err(to_js_err)?,
            Backend::Json(t) => t.send(&message).await.map_err(to_js_err)?,
            Backend::Ses(t) => t.send(&message).await.map_err(to_js_err)?,
        };

        // Plain objects, not JS `Map`s — serde-wasm-bindgen's default produces a `Map`
        // for struct fields, which `JSON.stringify`/`Response.json()` silently render
        // as `{}` (a `Map` has no own enumerable properties).
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        SentInfoOut::from(info)
            .serialize(&serializer)
            .map_err(|e| js_error(e.to_string()))
    }
}
