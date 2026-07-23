use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Where an attachment's bytes come from.
///
/// `Path` is only resolvable on native targets (behind the `fs` cargo feature) — WASM
/// runtimes have no filesystem, so callers there must supply `Bytes` directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttachmentContent {
    Bytes(Vec<u8>),
    /// Base64-encoded content, as accepted by nodemailer's `encoding: "base64"` option.
    Base64(String),
    #[cfg(feature = "fs")]
    Path(std::path::PathBuf),
    /// A remote URL, fetched with [`resolve_all`] (requires the `http` feature). Mirrors
    /// nodemailer's `attachments: [{ path: "https://..." }]`. Calling [`Attachment::resolve_bytes`]
    /// directly on an unresolved `Url` returns an error — fetch it first.
    Url(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: Option<String>,
    pub content: AttachmentContent,
    pub content_type: Option<String>,
    /// `attachment` (default) or `inline`.
    pub content_disposition: ContentDisposition,
    /// Content-ID for referencing this attachment as an embedded image via `cid:` in HTML.
    pub cid: Option<String>,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentDisposition {
    Attachment,
    Inline,
}

impl Attachment {
    pub fn from_bytes(filename: impl Into<String>, content: Vec<u8>) -> Self {
        Self {
            filename: Some(filename.into()),
            content: AttachmentContent::Bytes(content),
            content_type: None,
            content_disposition: ContentDisposition::Attachment,
            cid: None,
            headers: Vec::new(),
        }
    }

    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    pub fn inline_with_cid(mut self, cid: impl Into<String>) -> Self {
        self.content_disposition = ContentDisposition::Inline;
        self.cid = Some(cid.into());
        self
    }

    /// An attachment whose bytes are fetched from a URL — call [`resolve_all`] on the
    /// containing [`crate::Message`] (requires the `http` feature) before building/sending.
    pub fn from_url(filename: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            filename: Some(filename.into()),
            content: AttachmentContent::Url(url.into()),
            content_type: None,
            content_disposition: ContentDisposition::Attachment,
            cid: None,
            headers: Vec::new(),
        }
    }

    pub fn resolved_content_type(&self) -> String {
        if let Some(ct) = &self.content_type {
            return ct.clone();
        }
        if let Some(name) = &self.filename {
            return mime_guess::from_path(name)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
        }
        "application/octet-stream".to_string()
    }

    /// Resolves this attachment's bytes, decoding base64 payloads if needed.
    /// `Path` variants require the `fs` feature and native filesystem access.
    pub fn resolve_bytes(&self) -> Result<Vec<u8>> {
        match &self.content {
            AttachmentContent::Bytes(b) => Ok(b.clone()),
            AttachmentContent::Base64(s) => {
                use base64::{engine::general_purpose::STANDARD, Engine as _};
                STANDARD
                    .decode(s.as_bytes())
                    .map_err(|e| Error::Attachment(format!("invalid base64 content: {e}")))
            }
            #[cfg(feature = "fs")]
            AttachmentContent::Path(p) => std::fs::read(p).map_err(Error::from),
            AttachmentContent::Url(url) => Err(Error::Attachment(format!(
                "URL attachment not yet fetched, call resolve_all() first: {url}"
            ))),
        }
    }
}

/// Fetches every `Url` attachment/alternative on `message` and replaces it with its
/// downloaded `Bytes`, in place. Requires the `http` feature (pulls in `reqwest`, which
/// on `wasm32` calls through `fetch`, so this works in the browser too).
#[cfg(feature = "http")]
pub async fn resolve_all(message: &mut crate::Message) -> Result<()> {
    for attachment in message
        .attachments
        .iter_mut()
        .chain(message.alternatives.iter_mut())
    {
        if let AttachmentContent::Url(url) = &attachment.content {
            let bytes = fetch_url(url).await?;
            attachment.content = AttachmentContent::Bytes(bytes);
        }
    }
    Ok(())
}

/// Hard cap on a fetched URL attachment's size, to bound memory use against a huge or
/// malicious/misbehaving response (e.g. a server that lies about `Content-Length` or
/// streams indefinitely). 25MB is a generous ceiling for an email attachment — most
/// mail providers reject messages well below this after base64/MIME overhead anyway.
#[cfg(feature = "http")]
const MAX_FETCH_URL_BYTES: usize = 25 * 1024 * 1024;

/// Fetches `url` and returns its body, capped at [`MAX_FETCH_URL_BYTES`].
///
/// This does **not** perform any SSRF hardening (no private/link-local IP blocking,
/// no scheme/destination allowlist) — restricting which URLs are safe to fetch is a
/// host-application concern (the caller decides whether `url` came from a trusted
/// source). This function only guards against unbounded memory use once a fetch is
/// already permitted: it checks `Content-Length` up front when present, and reads the
/// body in chunks (rather than buffering the whole response via `response.bytes()`
/// unconditionally) so a response with no/incorrect `Content-Length` still can't grow
/// past the cap.
#[cfg(feature = "http")]
async fn fetch_url(url: &str) -> Result<Vec<u8>> {
    let mut response = reqwest::get(url)
        .await
        .map_err(|e| Error::Attachment(format!("failed to fetch {url}: {e}")))?;
    if !response.status().is_success() {
        return Err(Error::Attachment(format!(
            "failed to fetch {url}: HTTP {}",
            response.status()
        )));
    }
    if let Some(len) = response.content_length() {
        if len > MAX_FETCH_URL_BYTES as u64 {
            return Err(Error::Attachment(format!(
                "refusing to fetch {url}: response Content-Length {len} exceeds the {MAX_FETCH_URL_BYTES}-byte attachment size cap"
            )));
        }
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| Error::Attachment(format!("failed to read body of {url}: {e}")))?
    {
        if bytes.len() + chunk.len() > MAX_FETCH_URL_BYTES {
            return Err(Error::Attachment(format!(
                "refusing to fetch {url}: response exceeds the {MAX_FETCH_URL_BYTES}-byte attachment size cap"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
