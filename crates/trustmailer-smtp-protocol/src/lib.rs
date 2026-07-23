//! Pure SMTP protocol logic — command formatting, response parsing, dot-stuffing,
//! AUTH encoding, and the CRLF/`<>` injection checks from RFC 5321 §4.1.1. No I/O, no
//! socket type, no async runtime: this compiles on every target including `wasm32`,
//! so it's shared verbatim by [`trustmailer-smtp`](https://docs.rs/trustmailer-smtp)
//! (native, over `tokio`) and `trustmailer-smtp-cfworkers` (Cloudflare Workers, over
//! `cloudflare:sockets`) rather than reimplemented per backend.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use trustmailer_core::{Error, Result};

/// SMTP authentication mechanism to attempt after `EHLO`.
#[derive(Clone)]
pub enum Auth {
    Plain {
        username: String,
        password: String,
    },
    Login {
        username: String,
        password: String,
    },
    /// OAuth2 (XOAUTH2), e.g. for Gmail with an access token instead of a password.
    XOAuth2 {
        username: String,
        access_token: String,
    },
}

/// Hand-written rather than derived: the default `Debug` would print `password`/
/// `access_token` in plain text, and `{:?}` logging of an `Auth` (e.g. via
/// `tracing::debug!(?config)`) is a realistic troubleshooting reflex.
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::Plain { username, .. } => f
                .debug_struct("Auth::Plain")
                .field("username", username)
                .field("password", &"***REDACTED***")
                .finish(),
            Auth::Login { username, .. } => f
                .debug_struct("Auth::Login")
                .field("username", username)
                .field("password", &"***REDACTED***")
                .finish(),
            Auth::XOAuth2 { username, .. } => f
                .debug_struct("Auth::XOAuth2")
                .field("username", username)
                .field("access_token", &"***REDACTED***")
                .finish(),
        }
    }
}

impl Auth {
    pub fn mechanism_name(&self) -> &'static str {
        match self {
            Auth::Plain { .. } => "PLAIN",
            Auth::Login { .. } => "LOGIN",
            Auth::XOAuth2 { .. } => "XOAUTH2",
        }
    }

    /// The base64 blob sent as part of `AUTH <mechanism> <initial-response>` for the
    /// mechanisms that support an initial response (everything except LOGIN, which is
    /// challenge/response and has no combined single-command form).
    pub fn initial_response(&self) -> String {
        match self {
            Auth::Plain { username, password } => {
                let raw = format!("\0{username}\0{password}");
                STANDARD.encode(raw.as_bytes())
            }
            Auth::XOAuth2 {
                username,
                access_token,
            } => {
                let raw = format!("user={username}\x01auth=Bearer {access_token}\x01\x01");
                STANDARD.encode(raw.as_bytes())
            }
            Auth::Login { .. } => String::new(),
        }
    }

    pub fn encode(value: &str) -> String {
        STANDARD.encode(value.as_bytes())
    }
}

/// Strips (not escapes) every CR/LF, matching nodemailer's
/// `name.replace(/[\r\n]+/g, '')` — for values (the EHLO name) written directly into
/// an SMTP command, where a CRLF would let an attacker inject a second command.
pub fn strip_crlf(s: &str) -> String {
    s.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

/// Rejects (rather than sanitizes) an envelope `MAIL FROM`/`RCPT TO` address containing
/// CR, LF, `<`, or `>` — any of which could break out of `MAIL FROM:<...>` and inject a
/// second SMTP command. Matches nodemailer's `_setEnvelope` validation (`/[\r\n<>]/`).
pub fn validate_envelope_address(address: &str, role: &str) -> Result<()> {
    if address.is_empty()
        || address
            .chars()
            .any(|c| matches!(c, '\r' | '\n' | '<' | '>'))
    {
        return Err(Error::Transport(format!(
            "invalid {role} address: {address:?}"
        )));
    }
    Ok(())
}

pub fn ehlo_command(name: &str) -> String {
    format!("EHLO {name}")
}

pub fn mail_from_command(from: &str) -> String {
    format!("MAIL FROM:<{from}>")
}

pub fn rcpt_to_command(to: &str) -> String {
    format!("RCPT TO:<{to}>")
}

pub fn auth_command(auth: &Auth) -> String {
    format!("AUTH {} {}", auth.mechanism_name(), auth.initial_response())
}

/// Parses the capability list out of an EHLO response body (the greeting line itself
/// is skipped; each remaining line is one capability, e.g. `"AUTH PLAIN LOGIN"`).
pub fn parse_ehlo_capabilities(response_text: &str) -> Vec<String> {
    response_text
        .lines()
        .skip(1)
        .map(|l| l.to_string())
        .collect()
}

/// Dot-stuffs a message body for the `DATA` command per RFC 5321 §4.5.2: any line
/// starting with `.` gets a second `.` prepended, so the server doesn't mistake it for
/// the end-of-data marker.
pub fn dot_stuff(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut at_line_start = true;
    for &b in data {
        if at_line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_line_start = b == b'\n';
    }
    out
}

/// Hard cap on the number of continuation lines accumulated for a single (possibly
/// multi-line) SMTP response, so a malicious/misbehaving server that never sends a
/// final (`"250 ..."`) line can't grow client memory unbounded.
const MAX_RESPONSE_LINES: usize = 1000;

/// Hard cap on the total accumulated byte size of a single SMTP response's text.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// Incrementally assembles a (possibly multi-line, `250-...` continued) SMTP response
/// from complete lines (already stripped of the trailing CR/LF) fed one at a time —
/// backends read lines however fits their I/O model and drive this the same way.
#[derive(Debug, Default)]
pub struct ResponseParser {
    lines: Vec<String>,
    total_bytes: usize,
}

impl ResponseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one complete line (no trailing CR/LF). Returns `Some((code, text))` once
    /// the response is complete (a final line, `"250 ..."` rather than `"250-..."`).
    ///
    /// Never slices `line` by a raw byte offset without first confirming the bytes it
    /// spans are single-byte ASCII: SMTP status codes are always 3 ASCII digits
    /// followed by a 1-byte separator, but the *rest* of the line is attacker-
    /// controlled and may contain multi-byte UTF-8 that doesn't line up with those
    /// offsets — slicing by `&str` byte range in that case panics instead of erroring.
    pub fn feed_line(&mut self, line: &str) -> Result<Option<(u16, String)>> {
        let bytes = line.as_bytes();
        if bytes.len() < 4 || !bytes[..3].iter().all(u8::is_ascii_digit) {
            return Err(Error::Transport(format!(
                "malformed SMTP response line: {line:?}"
            )));
        }
        let sep = bytes[3];
        if sep != b' ' && sep != b'-' {
            return Err(Error::Transport(format!(
                "malformed SMTP response separator (expected ' ' or '-'): {line:?}"
            )));
        }
        // Safe: bytes[0..4] are all confirmed single-byte ASCII (3 digits + a ' '/'-'
        // separator), so byte offset 4 is guaranteed to be a `&str` char boundary.
        let code: u16 = line[..3]
            .parse()
            .map_err(|_| Error::Transport(format!("malformed SMTP status code: {line:?}")))?;
        let rest = &line[4..];

        if self.lines.len() >= MAX_RESPONSE_LINES
            || self.total_bytes + rest.len() > MAX_RESPONSE_BYTES
        {
            return Err(Error::Transport(
                "SMTP response exceeded maximum accumulated size".into(),
            ));
        }
        self.total_bytes += rest.len();
        self.lines.push(rest.to_string());
        if sep == b' ' {
            let text = self.lines.join("\n");
            self.lines.clear();
            self.total_bytes = 0;
            return Ok(Some((code, text)));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_stuffs_leading_dots() {
        assert_eq!(
            dot_stuff(b".hello\r\n.world\r\n"),
            b"..hello\r\n..world\r\n"
        );
        assert_eq!(dot_stuff(b"hello\r\nworld\r\n"), b"hello\r\nworld\r\n");
    }

    #[test]
    fn response_parser_handles_single_line() {
        let mut p = ResponseParser::new();
        assert_eq!(
            p.feed_line("250 OK").unwrap(),
            Some((250, "OK".to_string()))
        );
    }

    #[test]
    fn response_parser_handles_multiline() {
        let mut p = ResponseParser::new();
        assert_eq!(p.feed_line("250-fake.smtp").unwrap(), None);
        assert_eq!(p.feed_line("250-AUTH PLAIN LOGIN").unwrap(), None);
        assert_eq!(
            p.feed_line("250 PIPELINING").unwrap(),
            Some((250, "fake.smtp\nAUTH PLAIN LOGIN\nPIPELINING".to_string()))
        );
    }

    #[test]
    fn response_parser_resets_after_completion() {
        let mut p = ResponseParser::new();
        p.feed_line("220 hello").unwrap();
        assert_eq!(
            p.feed_line("250 OK").unwrap(),
            Some((250, "OK".to_string()))
        );
    }

    #[test]
    fn rejects_malformed_response_line() {
        let mut p = ResponseParser::new();
        assert!(p.feed_line("xx").is_err());
    }

    #[test]
    fn feed_line_rejects_rather_than_panics_on_multibyte_utf8_straddling_offset() {
        // "2€OK" — the 3-byte '€' (U+20AC, bytes 0xE2 0x82 0xAC) starts at byte index 1
        // and straddles byte offset 3, which used to make `line[..3]` panic ("byte
        // index 3 is not a char boundary") instead of returning a clean `Err`.
        let mut p = ResponseParser::new();
        let line = "2\u{20AC}OK";
        assert!(p.feed_line(line).is_err());
    }

    #[test]
    fn feed_line_rejects_unrecognized_separator_byte() {
        let mut p = ResponseParser::new();
        assert!(p.feed_line("250xOK").is_err());
    }

    #[test]
    fn feed_line_caps_runaway_multiline_response_accumulation() {
        let mut p = ResponseParser::new();
        let mut saw_error = false;
        for _ in 0..(MAX_RESPONSE_LINES + 10) {
            if p.feed_line("250-x").is_err() {
                saw_error = true;
                break;
            }
        }
        assert!(
            saw_error,
            "expected feed_line to cap an unbounded multi-line response"
        );
    }

    #[test]
    fn ehlo_capabilities_skip_greeting_line() {
        let caps = parse_ehlo_capabilities("fake.smtp\nAUTH PLAIN LOGIN\nPIPELINING");
        assert_eq!(
            caps,
            vec!["AUTH PLAIN LOGIN".to_string(), "PIPELINING".to_string()]
        );
    }

    #[test]
    fn ehlo_name_strips_crlf_injection() {
        let raw = "legit.host\r\nMAIL FROM:<attacker@evil.com>";
        assert_eq!(strip_crlf(raw), "legit.hostMAIL FROM:<attacker@evil.com>");
    }

    #[test]
    fn rejects_envelope_address_with_crlf_or_angle_brackets() {
        assert!(validate_envelope_address("a@b.com\r\nRCPT TO:<x>", "sender").is_err());
        assert!(validate_envelope_address("a@b.com>", "sender").is_err());
        assert!(validate_envelope_address("a@b.com", "sender").is_ok());
    }

    #[test]
    fn formats_commands() {
        assert_eq!(ehlo_command("mail.example.com"), "EHLO mail.example.com");
        assert_eq!(mail_from_command("a@b.com"), "MAIL FROM:<a@b.com>");
        assert_eq!(rcpt_to_command("c@d.com"), "RCPT TO:<c@d.com>");
    }

    #[test]
    fn auth_plain_initial_response_is_base64_of_nul_separated_triplet() {
        let auth = Auth::Plain {
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        let decoded = STANDARD.decode(auth.initial_response()).unwrap();
        assert_eq!(decoded, b"\0user\0pass");
    }
}
