use crate::address::{Address, IntoAddresses};
use crate::attachment::Attachment;
use crate::envelope::Envelope;
use crate::error::Result;
use crate::headers::{HeaderList, Priority};

/// The iCalendar `METHOD` property (RFC 5546) — the only 4 values iCal defines.
/// Previously a raw `String` interpolated straight into a `Content-Type` parameter,
/// which allowed MIME-parameter injection via a value containing `;`; an enum makes
/// injection impossible by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarMethod {
    #[default]
    Publish,
    Request,
    Reply,
    Cancel,
}

impl CalendarMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            CalendarMethod::Publish => "PUBLISH",
            CalendarMethod::Request => "REQUEST",
            CalendarMethod::Reply => "REPLY",
            CalendarMethod::Cancel => "CANCEL",
        }
    }
}

impl std::fmt::Display for CalendarMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A calendar invite/update, rendered as a `text/calendar` part — mirrors
/// nodemailer's `icalEvent` option.
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    /// `REQUEST` (invite), `REPLY`, `CANCEL`, or `PUBLISH` (default).
    pub method: CalendarMethod,
    /// Raw iCalendar (`.ics`) body.
    pub content: String,
    pub filename: String,
}

impl CalendarEvent {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            method: CalendarMethod::default(),
            content: content.into(),
            filename: "invite.ics".to_string(),
        }
    }

    pub fn with_method(mut self, method: CalendarMethod) -> Self {
        self.method = method;
        self
    }
}

/// One value for a `List-*` header — a URL (or bare domain, for `List-ID`) with an
/// optional human-readable comment. Ported from nodemailer's `list.*` option shape.
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub url: String,
    pub comment: Option<String>,
}

impl ListEntry {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            comment: None,
        }
    }

    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
}

impl From<&str> for ListEntry {
    fn from(url: &str) -> Self {
        ListEntry::new(url)
    }
}

/// A single outgoing email, mirroring nodemailer's `Mail` options object.
#[derive(Debug, Clone, Default)]
pub struct Message {
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub sender: Option<Address>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    /// Plain-text fallback shown by clients that strip HTML, distinct from `text`.
    pub watch_html: Option<String>,
    pub amp_html: Option<String>,
    pub headers: HeaderList,
    pub attachments: Vec<Attachment>,
    /// Extra top-level MIME alternatives beyond text/html (e.g. `text/calendar`).
    pub alternatives: Vec<Attachment>,
    pub envelope: Option<Envelope>,
    pub message_id: Option<String>,
    pub date: Option<time::OffsetDateTime>,
    pub priority: Option<Priority>,
    pub ical_event: Option<CalendarEvent>,
    /// `List-*` headers, keyed by suffix (`"unsubscribe"` → `List-Unsubscribe`, `"id"` →
    /// `List-ID`, etc). Any key is allowed, matching nodemailer's `list` option.
    pub list: Vec<(String, Vec<ListEntry>)>,
    /// Pre-built raw RFC 5322 message; when set, all other fields are ignored by the builder.
    pub raw: Option<Vec<u8>>,
}

impl Message {
    pub fn builder() -> MessageBuilder {
        MessageBuilder::default()
    }

    /// Resolves the effective envelope (explicit `envelope` field, falling back to
    /// `from`/`to`+`cc`+`bcc` headers), as nodemailer does. Any address groups in
    /// `to`/`cc`/`bcc` are flattened to their member mailboxes first, since the SMTP
    /// envelope has no notion of groups.
    pub fn effective_envelope(&self) -> Result<Envelope> {
        if let Some(env) = &self.envelope {
            return Ok(env.clone());
        }
        let from = self
            .sender
            .as_ref()
            .or(self.from.as_ref())
            .map(|a| a.address.clone())
            .unwrap_or_default();
        if from.is_empty() {
            return Err(crate::error::Error::NoSender);
        }

        // `Address::flatten_addresses` yields just the address strings (no cloned
        // name/group) since that's all an SMTP envelope recipient needs.
        let mut seen = std::collections::HashSet::new();
        let to: Vec<String> = Address::flatten_addresses(&self.to)
            .into_iter()
            .chain(Address::flatten_addresses(&self.cc))
            .chain(Address::flatten_addresses(&self.bcc))
            .filter(|a| !a.is_empty())
            // Order-preserving dedup: `to`/`cc`/`bcc` aren't sorted (and must not be,
            // to preserve original envelope ordering), so a plain `.dedup()` would only
            // catch *consecutive* duplicates. Track seen addresses instead.
            .filter(|a| seen.insert(a.clone()))
            .collect();
        if to.is_empty() {
            return Err(crate::error::Error::NoRecipients);
        }
        Ok(Envelope { from, to })
    }
}

#[derive(Debug, Clone, Default)]
pub struct MessageBuilder {
    msg: Message,
}

impl MessageBuilder {
    pub fn from(mut self, addr: impl Into<Address>) -> Self {
        self.msg.from = Some(addr.into());
        self
    }

    pub fn to(mut self, addrs: impl IntoAddresses) -> Self {
        self.msg.to = addrs.into_addresses();
        self
    }

    pub fn cc(mut self, addrs: impl IntoAddresses) -> Self {
        self.msg.cc = addrs.into_addresses();
        self
    }

    pub fn bcc(mut self, addrs: impl IntoAddresses) -> Self {
        self.msg.bcc = addrs.into_addresses();
        self
    }

    pub fn reply_to(mut self, addrs: impl IntoAddresses) -> Self {
        self.msg.reply_to = addrs.into_addresses();
        self
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.msg.subject = Some(subject.into());
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.msg.text = Some(text.into());
        self
    }

    pub fn html(mut self, html: impl Into<String>) -> Self {
        self.msg.html = Some(html.into());
        self
    }

    /// Plain-text-ish fallback shown by clients that strip full HTML but allow simple
    /// markup (e.g. Apple Watch) — rendered as its own `text/watch-html` alternative.
    pub fn watch_html(mut self, watch_html: impl Into<String>) -> Self {
        self.msg.watch_html = Some(watch_html.into());
        self
    }

    /// AMP4Email body, rendered as a `text/x-amp-html` alternative.
    pub fn amp_html(mut self, amp_html: impl Into<String>) -> Self {
        self.msg.amp_html = Some(amp_html.into());
        self
    }

    pub fn attachment(mut self, attachment: Attachment) -> Self {
        self.msg.attachments.push(attachment);
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.msg.headers.push(name, value);
        self
    }

    pub fn priority(mut self, priority: Priority) -> Self {
        self.msg.priority = Some(priority);
        self
    }

    pub fn envelope(mut self, envelope: Envelope) -> Self {
        self.msg.envelope = Some(envelope);
        self
    }

    pub fn message_id(mut self, id: impl Into<String>) -> Self {
        self.msg.message_id = Some(id.into());
        self
    }

    pub fn ical_event(mut self, event: CalendarEvent) -> Self {
        self.msg.ical_event = Some(event);
        self
    }

    /// Sets a `List-*` header (`key` is the suffix: `"unsubscribe"`, `"help"`, `"id"`, ...),
    /// mirroring nodemailer's `list` option. Multiple entries are comma-joined into one
    /// header. `List-ID` gets the special RFC 2919 `<domain>` (rather than `<scheme:url>`) form.
    pub fn list(mut self, key: impl Into<String>, entries: Vec<ListEntry>) -> Self {
        self.msg.list.push((key.into(), entries));
        self
    }

    /// Sets `List-Unsubscribe` to one or more URLs, per RFC 2369.
    pub fn list_unsubscribe(self, entries: Vec<ListEntry>) -> Self {
        self.list("unsubscribe", entries)
    }

    /// Adds `List-Unsubscribe-Post: List-Unsubscribe=One-Click`, per RFC 8058 — required
    /// by Gmail/Yahoo bulk-sender rules alongside a `List-Unsubscribe` URL.
    pub fn list_unsubscribe_post(mut self) -> Self {
        self.msg
            .headers
            .push("List-Unsubscribe-Post", "List-Unsubscribe=One-Click");
        self
    }

    pub fn build(self) -> Message {
        self.msg
    }
}

impl From<&str> for Address {
    fn from(s: &str) -> Self {
        let mut v = crate::address::parse_address_list(s);
        if v.len() == 1 {
            v.pop().unwrap()
        } else {
            Address::new(s)
        }
    }
}

impl From<String> for Address {
    fn from(s: String) -> Self {
        Address::from(s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression: `to.dedup()` only collapsed *consecutive* duplicates. `to`/`cc`/`bcc`
    // aren't (and must not be) sorted first, since the envelope needs to preserve
    // original recipient order — so a non-adjacent duplicate (recipient repeated across
    // `to` and `bcc`, with something else in between) used to survive.
    #[test]
    fn effective_envelope_dedups_non_adjacent_recipients() {
        let msg = Message::builder()
            .from("sender@example.com")
            .to(vec!["a@example.com", "b@example.com"])
            .cc(vec!["c@example.com"])
            .bcc(vec!["a@example.com"]) // duplicate of the first `to`, not adjacent to it
            .build();
        let envelope = msg.effective_envelope().unwrap();
        assert_eq!(
            envelope.to,
            vec![
                "a@example.com".to_string(),
                "b@example.com".to_string(),
                "c@example.com".to_string(),
            ]
        );
    }

    #[test]
    fn effective_envelope_preserves_original_order() {
        let msg = Message::builder()
            .from("sender@example.com")
            .to(vec!["z@example.com", "a@example.com"])
            .build();
        let envelope = msg.effective_envelope().unwrap();
        // Order must be preserved (not sorted) — envelope semantics depend on it.
        assert_eq!(
            envelope.to,
            vec!["z@example.com".to_string(), "a@example.com".to_string()]
        );
    }

    // Regression: empty/absent `from` used to silently produce `Envelope.from == ""`
    // instead of erroring, symmetric with the existing empty-`to` -> `NoRecipients` check.
    #[test]
    fn effective_envelope_errors_on_missing_from() {
        let msg = Message::builder().to("a@example.com").build();
        let err = msg.effective_envelope().unwrap_err();
        assert!(matches!(err, crate::error::Error::NoSender));
    }

    #[test]
    fn effective_envelope_errors_on_empty_to() {
        let msg = Message::builder().from("sender@example.com").build();
        let err = msg.effective_envelope().unwrap_err();
        assert!(matches!(err, crate::error::Error::NoRecipients));
    }
}
