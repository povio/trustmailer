use serde::{Deserialize, Serialize};

/// SMTP envelope (`MAIL FROM` / `RCPT TO`), which may differ from the visible
/// `From`/`To` headers — e.g. for VERP bounce handling.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Envelope {
    pub from: String,
    pub to: Vec<String>,
}
