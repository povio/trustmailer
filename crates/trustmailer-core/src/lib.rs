//! Core building blocks for trustmailer: address parsing, the RFC 5322/MIME message
//! builder, the [`Transport`] trait every backend implements, and shared types
//! (attachments, envelopes, headers). This crate has no I/O and compiles to `wasm32`
//! as-is, so it's safe to depend on from any runtime binding.

pub mod address;
pub mod attachment;
pub mod envelope;
pub mod error;
pub mod headers;
pub mod message;
pub mod mime;
pub mod transport;
pub mod well_known;

pub use address::{Address, IntoAddresses};
#[cfg(feature = "http")]
pub use attachment::resolve_all;
pub use attachment::{Attachment, AttachmentContent, ContentDisposition};
pub use envelope::Envelope;
pub use error::{Error, Result};
pub use headers::{HeaderList, Priority};
pub use message::{CalendarEvent, ListEntry, Message, MessageBuilder};
pub use transport::{SentInfo, Transport};
