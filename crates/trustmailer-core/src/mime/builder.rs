use super::encode::{encode_header_value, generate_boundary, generate_message_id};
use super::node::{MimeNode, TransferEncoding};
use crate::address::Address;
use crate::attachment::{Attachment, ContentDisposition};
use crate::error::Result;
use crate::message::{ListEntry, Message};

/// Builds the full raw RFC 5322 / MIME byte stream for a [`Message`].
///
/// `hostname` is used to generate a `Message-ID` when the message doesn't set one.
pub fn build(message: &Message, hostname: &str) -> Result<Vec<u8>> {
    if let Some(raw) = &message.raw {
        return Ok(raw.clone());
    }

    let mut content_nodes = Vec::new();
    if let Some(text) = &message.text {
        content_nodes.push(text_leaf("text/plain; charset=utf-8", text));
    }
    if let Some(watch) = &message.watch_html {
        content_nodes.push(text_leaf("text/watch-html; charset=utf-8", watch));
    }
    if let Some(amp) = &message.amp_html {
        content_nodes.push(text_leaf("text/x-amp-html; charset=utf-8", amp));
    }
    if let Some(html) = &message.html {
        content_nodes.push(text_leaf("text/html; charset=utf-8", html));
    }
    for alt in &message.alternatives {
        content_nodes.push(attachment_leaf(alt)?);
    }
    if let Some(event) = &message.ical_event {
        content_nodes.push(calendar_leaf(event));
    }

    if content_nodes.is_empty() {
        content_nodes.push(text_leaf("text/plain; charset=utf-8", ""));
    }

    let mut body = if content_nodes.len() == 1 {
        content_nodes.into_iter().next().unwrap()
    } else {
        let boundary = generate_boundary();
        let headers = vec![(
            "Content-Type".to_string(),
            format!("multipart/alternative; boundary=\"{boundary}\""),
        )];
        MimeNode::multipart(headers, boundary, content_nodes)
    };

    let (inline, regular): (Vec<&Attachment>, Vec<&Attachment>) = message
        .attachments
        .iter()
        .partition(|a| a.content_disposition == ContentDisposition::Inline && a.cid.is_some());

    if !inline.is_empty() {
        let mut children = vec![body];
        for a in &inline {
            children.push(attachment_leaf(a)?);
        }
        let boundary = generate_boundary();
        let headers = vec![(
            "Content-Type".to_string(),
            format!("multipart/related; boundary=\"{boundary}\""),
        )];
        body = MimeNode::multipart(headers, boundary, children);
    }

    if !regular.is_empty() {
        let mut children = vec![body];
        for a in &regular {
            children.push(attachment_leaf(a)?);
        }
        let boundary = generate_boundary();
        let headers = vec![(
            "Content-Type".to_string(),
            format!("multipart/mixed; boundary=\"{boundary}\""),
        )];
        body = MimeNode::multipart(headers, boundary, children);
    }

    prepend_root_headers(&mut body, message, hostname);

    Ok(body.render())
}

fn calendar_leaf(event: &crate::message::CalendarEvent) -> MimeNode {
    // `event.method` is a `CalendarMethod` enum (not a raw string), so this can only
    // ever render one of the 4 valid iCal method tokens — no MIME-parameter injection
    // via a value containing `;` is possible here by construction.
    let content_type = format!("text/calendar; charset=utf-8; method={}", event.method);
    let headers = vec![
        ("Content-Type".to_string(), content_type),
        (
            "Content-Transfer-Encoding".to_string(),
            "base64".to_string(),
        ),
    ];
    MimeNode::leaf(
        headers,
        event.content.as_bytes().to_vec(),
        TransferEncoding::Base64,
    )
}

fn text_leaf(content_type: &str, text: &str) -> MimeNode {
    let headers = vec![
        ("Content-Type".to_string(), content_type.to_string()),
        (
            "Content-Transfer-Encoding".to_string(),
            "quoted-printable".to_string(),
        ),
    ];
    MimeNode::leaf(
        headers,
        text.as_bytes().to_vec(),
        TransferEncoding::QuotedPrintable,
    )
}

fn attachment_leaf(a: &Attachment) -> Result<MimeNode> {
    let bytes = a.resolve_bytes()?;
    let content_type = a.resolved_content_type();
    let is_text = content_type.starts_with("text/");
    let mut headers = Vec::new();

    let ct_with_name = match &a.filename {
        Some(name) => format!("{content_type}; {}", format_filename_param("name", name)),
        None => content_type.clone(),
    };
    headers.push(("Content-Type".to_string(), ct_with_name));

    let transfer_encoding = if is_text {
        TransferEncoding::QuotedPrintable
    } else {
        TransferEncoding::Base64
    };
    let cte_name = match transfer_encoding {
        TransferEncoding::QuotedPrintable => "quoted-printable",
        TransferEncoding::Base64 => "base64",
        TransferEncoding::SevenBit => "7bit",
    };
    headers.push((
        "Content-Transfer-Encoding".to_string(),
        cte_name.to_string(),
    ));

    let disposition = match a.content_disposition {
        ContentDisposition::Attachment => "attachment",
        ContentDisposition::Inline => "inline",
    };
    let disposition_value = match &a.filename {
        Some(name) => format!("{disposition}; {}", format_filename_param("filename", name)),
        None => disposition.to_string(),
    };
    headers.push(("Content-Disposition".to_string(), disposition_value));

    if let Some(cid) = &a.cid {
        headers.push(("Content-ID".to_string(), format!("<{cid}>")));
    }
    for (name, value) in &a.headers {
        headers.push((name.clone(), value.clone()));
    }

    Ok(MimeNode::leaf(headers, bytes, transfer_encoding))
}

/// Renders a `name=`/`filename=` MIME parameter. Plain ASCII values with no
/// characters that would need quoting/escaping get the simple `name="value"` form;
/// anything else (non-ASCII, or ASCII containing a literal `"` that can't be safely
/// embedded in a quoted-string) uses the RFC 2231 extended form: `name*=UTF-8''<percent-encoded>`.
///
/// Unlike nodemailer, this doesn't split long values into `name*0*=`/`name*1*=`
/// continuation chunks — single-chunk RFC 2231 is what every mainstream mail client
/// actually parses, and chunking mainly matters for very long (100+ char) filenames.
fn format_filename_param(param: &str, value: &str) -> String {
    let needs_extended_form = value.chars().any(|c| c == '"' || c.is_control());
    if value.is_ascii() && !needs_extended_form {
        format!("{param}=\"{value}\"")
    } else {
        format!("{param}*=UTF-8''{}", percent_encode_rfc2231(value))
    }
}

/// Percent-encodes everything except RFC 2231/5987 `attr-char` (roughly:
/// alphanumerics and a handful of punctuation marks known to be safe unencoded).
fn percent_encode_rfc2231(value: &str) -> String {
    const SAFE: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$&+-.^_`|~";
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if SAFE.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn prepend_root_headers(root: &mut MimeNode, message: &Message, hostname: &str) {
    let mut headers = Vec::new();
    headers.push(("MIME-Version".to_string(), "1.0".to_string()));

    if let Some(from) = &message.from {
        headers.push(("From".to_string(), from.to_header_value()));
    }
    push_address_header(&mut headers, "To", &message.to);
    push_address_header(&mut headers, "Cc", &message.cc);
    push_address_header(&mut headers, "Reply-To", &message.reply_to);

    if let Some(subject) = &message.subject {
        headers.push(("Subject".to_string(), encode_header_value(subject)));
    }
    if let Some(in_reply_to) = &message.in_reply_to {
        headers.push(("In-Reply-To".to_string(), in_reply_to.clone()));
    }
    if !message.references.is_empty() {
        headers.push(("References".to_string(), message.references.join(" ")));
    }
    if let Some(priority) = message.priority {
        headers.push(("X-Priority".to_string(), priority.x_priority().to_string()));
        headers.push(("Importance".to_string(), priority.importance().to_string()));
    }

    let date = message.date.unwrap_or_else(rfc5322_now);
    headers.push(("Date".to_string(), format_rfc5322(date)));

    let message_id = message
        .message_id
        .clone()
        .unwrap_or_else(|| generate_message_id(hostname));
    headers.push(("Message-ID".to_string(), message_id));

    for (key, entries) in &message.list {
        if entries.is_empty() {
            continue;
        }
        let header_name = format!("List-{}", capitalize(key));
        let is_id = key.eq_ignore_ascii_case("id");
        let value = entries
            .iter()
            .map(|e| format_list_entry(e, is_id))
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !value.is_empty() {
            headers.push((header_name, value));
        }
    }

    for (name, value) in &message.headers.0 {
        headers.push((name.clone(), encode_header_value(value)));
    }

    headers.extend(std::mem::take(&mut root.headers));
    root.headers = headers;
}

fn push_address_header(headers: &mut Vec<(String, String)>, name: &str, addrs: &[Address]) {
    if addrs.is_empty() {
        return;
    }
    let joined = addrs
        .iter()
        .map(|a| a.to_header_value())
        .collect::<Vec<_>>()
        .join(", ");
    headers.push((name.to_string(), joined));
}

fn rfc5322_now() -> time::OffsetDateTime {
    time::OffsetDateTime::now_utc()
}

fn format_rfc5322(dt: time::OffsetDateTime) -> String {
    use time::format_description::well_known::Rfc2822;
    dt.format(&Rfc2822).unwrap_or_else(|_| dt.to_string())
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Wraps a URL as `<scheme:url>`, inferring `mailto:` for a bare `user@host`, and
/// falling back to `http://` for a bare domain. Ported from nodemailer's
/// `MailMessage._formatListUrl`.
fn format_list_url(url: &str) -> String {
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '<' && *c != '>')
        .collect();
    let lower = cleaned.to_ascii_lowercase();
    if lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.starts_with("mailto:")
        || lower.starts_with("ftp:")
    {
        return format!("<{cleaned}>");
    }
    let at_count = cleaned.matches('@').count();
    if at_count == 1 && !cleaned.starts_with('@') && !cleaned.ends_with('@') {
        return format!("<mailto:{cleaned}>");
    }
    format!("<http://{cleaned}>")
}

/// `List-ID` wants a bare `<domain>` identifier rather than `<scheme:url>` — strip the
/// scheme prefix `format_list_url` added (or that was already present).
fn strip_scheme_for_list_id(formatted: &str) -> String {
    let Some(rest) = formatted.strip_prefix('<') else {
        return formatted.to_string();
    };
    let Some(colon) = rest.find(':') else {
        return formatted.to_string();
    };
    let after_colon = &rest[colon + 1..];
    let after_slashes = after_colon.trim_start_matches('/');
    format!("<{after_slashes}")
}

fn is_ascii_printable(s: &str) -> bool {
    s.chars().all(|c| (' '..='~').contains(&c))
}

fn format_list_entry(entry: &ListEntry, is_id: bool) -> String {
    if entry.url.is_empty() {
        return String::new();
    }
    let formatted_url = format_list_url(&entry.url);

    if is_id {
        let bare = strip_scheme_for_list_id(&formatted_url);
        return match &entry.comment {
            Some(comment) if !comment.is_empty() => {
                let comment = if is_ascii_printable(comment) {
                    format!("\"{comment}\"")
                } else {
                    encode_header_value(comment)
                };
                format!("{comment} {bare}")
            }
            _ => bare,
        };
    }

    match &entry.comment {
        Some(comment) if !comment.is_empty() => {
            let comment = if is_ascii_printable(comment) {
                comment.clone()
            } else {
                encode_header_value(comment)
            };
            format!("{formatted_url} ({comment})")
        }
        _ => formatted_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{CalendarEvent, Message};

    #[test]
    fn builds_simple_text_message() {
        let msg = Message::builder()
            .from("Alice <alice@example.com>")
            .to("bob@example.com")
            .subject("Hello")
            .text("Hi Bob")
            .build();
        let raw = build(&msg, "example.com").unwrap();
        let s = String::from_utf8(raw).unwrap();
        assert!(s.contains("From: Alice <alice@example.com>"));
        assert!(s.contains("To: bob@example.com"));
        assert!(s.contains("Subject: Hello"));
        assert!(s.contains("Hi Bob"));
        assert!(s.contains("MIME-Version: 1.0"));
    }

    #[test]
    fn builds_multipart_alternative_for_text_and_html() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("Hi")
            .text("plain")
            .html("<b>html</b>")
            .build();
        let raw = build(&msg, "example.com").unwrap();
        let s = String::from_utf8(raw).unwrap();
        assert!(s.contains("multipart/alternative"));
        assert!(s.contains("text/plain"));
        assert!(s.contains("text/html"));
    }

    #[test]
    fn builds_mixed_for_attachments() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("Hi")
            .text("plain")
            .attachment(
                Attachment::from_bytes("test.txt", b"hello".to_vec())
                    .with_content_type("text/plain"),
            )
            .build();
        let raw = build(&msg, "example.com").unwrap();
        let s = String::from_utf8(raw).unwrap();
        assert!(s.contains("multipart/mixed"));
        assert!(s.contains("filename=\"test.txt\""));
    }

    // The following tests are behavioral ports of test/mail-composer/mail-composer-test.js
    // and test/mime-node/mime-node-test.js from nodemailer. They assert the same
    // *structural* outcomes (which multipart wrapper is chosen, alternative ordering,
    // header propagation) rather than byte-exact output — trustmailer's serialization
    // (boundary format, default transfer encodings, header ordering) intentionally
    // differs from nodemailer's, so a literal string diff isn't meaningful here.

    fn part_order(haystack: &str, needles: &[&str]) -> bool {
        let mut from = 0;
        for needle in needles {
            match haystack[from..].find(needle) {
                Some(pos) => from += pos + needle.len(),
                None => return false,
            }
        }
        true
    }

    #[test]
    fn mixed_structure_with_multiple_attachments() {
        let msg = Message::builder()
            .text("")
            .attachment(Attachment::from_bytes("a.txt", b"abc".to_vec()))
            .attachment(Attachment::from_bytes("b.txt", b"def".to_vec()))
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("filename=\"a.txt\""));
        assert!(raw.contains("filename=\"b.txt\""));
    }

    #[test]
    fn alternative_order_text_watch_html_html() {
        let msg = Message::builder()
            .text("abc")
            .watch_html("ghi")
            .html("def")
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(part_order(
            &raw,
            &["text/plain", "text/watch-html", "text/html"]
        ));
    }

    #[test]
    fn alternative_order_text_amp_html() {
        let msg = Message::builder()
            .text("abc")
            .amp_html("ghi")
            .html("def")
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(part_order(
            &raw,
            &["text/plain", "text/x-amp-html", "text/html"]
        ));
    }

    #[test]
    fn alternative_includes_ical_event_after_html() {
        let msg = Message::builder()
            .text("abc")
            .html("def")
            .ical_event(CalendarEvent::new("ghi"))
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(part_order(
            &raw,
            &["text/plain", "text/html", "text/calendar"]
        ));
        assert!(raw.contains("method=PUBLISH"));
    }

    #[test]
    fn ical_event_method_renders_uppercase_token() {
        let msg = Message::builder()
            .text("abc")
            .ical_event(
                CalendarEvent::new("ghi").with_method(crate::message::CalendarMethod::Request),
            )
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(raw.contains("method=REQUEST"));
    }

    // Regression: `method` is a `CalendarMethod` enum, not a raw string, so a value
    // like `PUBLISH; boundary=evil` can no longer be constructed at all — MIME
    // parameter injection into the `Content-Type` header is impossible by construction.
    #[test]
    fn ical_event_method_cannot_inject_mime_parameters() {
        for method in [
            crate::message::CalendarMethod::Publish,
            crate::message::CalendarMethod::Request,
            crate::message::CalendarMethod::Reply,
            crate::message::CalendarMethod::Cancel,
        ] {
            let msg = Message::builder()
                .text("abc")
                .ical_event(CalendarEvent::new("ghi").with_method(method))
                .build();
            let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
            let expected = format!("method={method}");
            assert!(raw.contains(&expected), "raw: {raw}");
        }
    }

    #[test]
    fn related_structure_for_html_with_cid_attachments() {
        let msg = Message::builder()
            .html("<img src=cid:aaa>")
            .attachment(Attachment::from_bytes("a.png", b"abc".to_vec()).inline_with_cid("aaa"))
            .attachment(Attachment::from_bytes("b.png", b"def".to_vec()).inline_with_cid("bbb"))
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(raw.contains("multipart/related"));
        assert!(raw.contains("Content-ID: <aaa>"));
        assert!(raw.contains("Content-ID: <bbb>"));
        assert!(!raw.contains("multipart/mixed"));
    }

    #[test]
    fn discards_bcc_from_headers() {
        let msg = Message::builder()
            .from("test1@example.com")
            .to("test2@example.com")
            .bcc("test3@example.com")
            .text("def")
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(!raw.contains("test3@example.com"));
        assert!(!raw.to_lowercase().contains("bcc:"));
    }

    #[test]
    fn bcc_is_still_present_in_envelope() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .bcc("c@example.com")
            .text("x")
            .build();
        let envelope = msg.effective_envelope().unwrap();
        assert!(envelope.to.contains(&"c@example.com".to_string()));
    }

    #[test]
    fn custom_attachment_headers_are_propagated() {
        let attachment = Attachment {
            headers: vec![("X-Test-1".to_string(), "12345".to_string())],
            ..Attachment::from_bytes("test.txt", b"test".to_vec())
        };
        let msg = Message::builder()
            .text("abc")
            .attachment(attachment)
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(raw.contains("X-Test-1: 12345"));
    }

    #[test]
    fn non_ascii_attachment_filename_uses_rfc2231_extended_param() {
        let msg = Message::builder()
            .text("abc")
            .attachment(Attachment::from_bytes("ÕÄÖÜ.txt", b"test".to_vec()))
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        // RFC 2231: filename*=UTF-8''<percent-encoded UTF-8 bytes>.
        assert!(
            raw.contains("filename*=UTF-8''%C3%95%C3%84%C3%96%C3%9C.txt"),
            "raw: {raw}"
        );
        assert!(!raw.contains("filename=\"ÕÄÖÜ"));
    }

    // Ported from mail-composer-test.js "should encode filename": a filename with a
    // literal double quote can't safely go in a quoted-string, so even though it's
    // pure ASCII it still needs the RFC 2231 extended form.
    #[test]
    fn ascii_filename_with_embedded_quote_uses_rfc2231() {
        let msg = Message::builder()
            .text("abc")
            .attachment(Attachment::from_bytes(
                "\"test\" it is.txt",
                b"test".to_vec(),
            ))
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(
            raw.contains("filename*=UTF-8''%22test%22%20it%20is.txt"),
            "raw: {raw}"
        );
    }

    #[test]
    fn plain_ascii_filename_uses_simple_quoted_form() {
        let msg = Message::builder()
            .text("abc")
            .attachment(Attachment::from_bytes("report.pdf", b"test".to_vec()))
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(raw.contains("filename=report.pdf") || raw.contains("filename=\"report.pdf\""));
    }

    #[test]
    fn content_node_with_only_attachment_and_no_text_defaults_to_empty_body() {
        let msg = Message::builder()
            .attachment(Attachment::from_bytes("a.txt", b"abc".to_vec()))
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("text/plain"));
    }

    #[test]
    fn alternatives_field_adds_extra_mime_parts() {
        let mut msg = Message::builder().text("abc").html("def").build();
        msg.alternatives.push(
            Attachment::from_bytes("cal.ics", b"BEGIN:VCALENDAR".to_vec())
                .with_content_type("text/calendar"),
        );
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(part_order(
            &raw,
            &["text/plain", "text/html", "text/calendar"]
        ));
    }

    // Ported from test/nodemailer/list-headers-test.js — regression tests for
    // GHSA-268h-hp4c-crq3 (CRLF in a `list.*.comment` must not inject a header line).

    #[test]
    fn list_comment_crlf_does_not_inject_a_header() {
        for key in [
            "help",
            "unsubscribe",
            "subscribe",
            "post",
            "owner",
            "archive",
            "id",
        ] {
            let entry = ListEntry::new(if key == "id" {
                "example.test".to_string()
            } else {
                format!("https://example.test/{key}")
            })
            .with_comment(format!("comment\r\nX-Injected-{key}: yes"));
            let msg = Message::builder()
                .from("sender@example.test")
                .to("recipient@example.test")
                .subject(format!("list {key}"))
                .text("body")
                .list(key, vec![entry])
                .build();
            let raw = String::from_utf8(build(&msg, "example.test").unwrap()).unwrap();

            assert!(
                !raw.contains(&format!("\r\nX-Injected-{key}: yes")),
                "CRLF injected a standalone header for list.{key}"
            );
            let header_name = format!("List-{}:", capitalize(key));
            assert!(
                raw.lines().any(|l| l.starts_with(&header_name)),
                "missing {header_name} header"
            );
        }
    }

    #[test]
    fn list_id_formats_a_bare_domain() {
        let msg = Message::builder()
            .from("sender@example.test")
            .to("recipient@example.test")
            .subject("list id")
            .text("body")
            .list("id", vec![ListEntry::new("mylist.example.test")])
            .build();
        let raw = String::from_utf8(build(&msg, "example.test").unwrap()).unwrap();
        let line = raw
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("list-id:"))
            .unwrap();
        assert!(
            line.contains("<mylist.example.test>"),
            "expected bare domain form, got: {line}"
        );
        assert!(
            !line.contains("http://"),
            "scheme prefix must be stripped: {line}"
        );
    }

    #[test]
    fn list_id_strips_scheme_from_url_form_and_keeps_comment() {
        let msg = Message::builder()
            .from("sender@example.test")
            .to("recipient@example.test")
            .subject("list id")
            .text("body")
            .list(
                "id",
                vec![ListEntry::new("https://mylist.example.test").with_comment("My List")],
            )
            .build();
        let raw = String::from_utf8(build(&msg, "example.test").unwrap()).unwrap();
        let line = raw
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("list-id:"))
            .unwrap();
        assert!(
            line.contains("\"My List\" <mylist.example.test>"),
            "expected comment + bare domain, got: {line}"
        );
    }

    #[test]
    fn list_unsubscribe_keeps_a_benign_comment() {
        let msg = Message::builder()
            .from("sender@example.test")
            .to("recipient@example.test")
            .subject("benign")
            .text("body")
            .list_unsubscribe(vec![
                ListEntry::new("https://example.test/u").with_comment("Unsubscribe here")
            ])
            .build();
        let raw = String::from_utf8(build(&msg, "example.test").unwrap()).unwrap();
        let line = raw
            .lines()
            .find(|l| l.starts_with("List-Unsubscribe:"))
            .unwrap();
        assert!(line.contains("(Unsubscribe here)"));
    }

    #[test]
    fn list_unsubscribe_post_header_is_present() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .text("x")
            .list_unsubscribe(vec![ListEntry::new("https://example.test/u")])
            .list_unsubscribe_post()
            .build();
        let raw = String::from_utf8(build(&msg, "example.test").unwrap()).unwrap();
        assert!(raw.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"));
    }

    // General CRLF-injection regression: any header value (not just list comments) must
    // not be able to terminate the header early and inject a new one.
    #[test]
    fn crlf_in_subject_does_not_inject_a_header() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("hi\r\nX-Injected: evil")
            .text("body")
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(!raw.contains("\r\nX-Injected: evil"));
    }

    #[test]
    fn crlf_in_custom_header_does_not_inject_a_header() {
        let msg = Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .text("body")
            .header("X-Custom", "value\r\nX-Injected: evil")
            .build();
        let raw = String::from_utf8(build(&msg, "example.com").unwrap()).unwrap();
        assert!(!raw.contains("\r\nX-Injected: evil"));
    }
}
