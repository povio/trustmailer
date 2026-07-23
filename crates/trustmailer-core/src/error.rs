use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("no recipients defined (to/cc/bcc/envelope.to)")]
    NoRecipients,

    #[error("no sender defined (from/sender/envelope.from)")]
    NoSender,

    #[error("attachment error: {0}")]
    Attachment(String),

    #[error("mime build error: {0}")]
    Mime(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("dkim error: {0}")]
    Dkim(String),
}

pub type Result<T> = std::result::Result<T, Error>;
