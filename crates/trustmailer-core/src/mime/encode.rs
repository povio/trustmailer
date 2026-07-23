use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::distr::Alphanumeric;
use rand::RngExt;

/// Wraps base64 output at 76 chars per line separated by CRLF, as required for MIME bodies.
pub fn base64_wrap(bytes: &[u8]) -> String {
    let encoded = STANDARD.encode(bytes);
    wrap_lines(&encoded, 76)
}

fn wrap_lines(s: &str, width: usize) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / width * 2);
    for (i, chunk) in s.as_bytes().chunks(width).enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(std::str::from_utf8(chunk).expect("base64 alphabet is ascii"));
    }
    out
}

/// Quoted-printable encodes text per RFC 2045, with soft line breaks at 76 columns.
/// Existing CRLF line endings are preserved verbatim.
pub fn quoted_printable_encode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut line_len = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            out.push_str("\r\n");
            line_len = 0;
            i += 2;
            continue;
        }
        if bytes[i] == b'\n' {
            out.push_str("\r\n");
            line_len = 0;
            i += 1;
            continue;
        }
        let b = bytes[i];
        let needs_escape = b >= 0x7f || b < 0x20 && b != b'\t' || b == b'=';
        let printable_space_or_tab = b == b' ' || b == b'\t';

        if line_len >= 73 {
            out.push_str("=\r\n");
            line_len = 0;
        }

        if needs_escape {
            out.push_str(&format!("={:02X}", b));
            line_len += 3;
        } else if printable_space_or_tab {
            // Trailing whitespace before a line break must be escaped; peek ahead.
            let at_eol = matches!(bytes.get(i + 1), None | Some(b'\r') | Some(b'\n'));
            if at_eol {
                out.push_str(&format!("={:02X}", b));
                line_len += 3;
            } else {
                out.push(b as char);
                line_len += 1;
            }
        } else {
            out.push(b as char);
            line_len += 1;
        }
        i += 1;
    }
    out
}

/// Encodes a header phrase (e.g. a display name) per RFC 2047 if it contains
/// non-ASCII bytes; otherwise returns it as a quoted string when needed, or bare.
pub fn encode_header_phrase(phrase: &str) -> String {
    if phrase.is_ascii() {
        if phrase.chars().any(|c| ",;<>\"".contains(c)) {
            // Escape `\` before `"` — order matters. Escaping `"` first and then
            // escaping the resulting `\` characters would double-escape the
            // backslashes just inserted for the quotes; escaping `\` first (so a
            // literal `\` becomes `\\`) and then quoting `"` (`"` becomes `\"`)
            // produces a properly-terminated RFC 5322 quoted-string in both cases.
            // Without escaping `\`, a phrase ending in an odd run of backslashes
            // (e.g. `Name\` before the closing quote is appended) would leave the
            // quoted-string unterminated, letting everything after on the header
            // line be read as still-quoted.
            let escaped = phrase.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        } else {
            phrase.to_string()
        }
    } else {
        format!("=?UTF-8?B?{}?=", STANDARD.encode(phrase.as_bytes()))
    }
}

/// Encodes an arbitrary header *value* (not a structured phrase) per RFC 2047 when
/// it contains non-ASCII characters, e.g. for `Subject`.
pub fn encode_header_value(value: &str) -> String {
    if value.is_ascii() {
        value.to_string()
    } else {
        format!("=?UTF-8?B?{}?=", STANDARD.encode(value.as_bytes()))
    }
}

/// Generates a random MIME multipart boundary string.
pub fn generate_boundary() -> String {
    let mut rng = rand::rng();
    let suffix: String = (0..24)
        .map(|_| char::from(rng.sample(Alphanumeric)))
        .collect();
    format!("--trustmailer-{suffix}")
}

/// Generates a Message-ID local part / value, e.g. `<uuid@hostname>`.
pub fn generate_message_id(hostname: &str) -> String {
    format!("<{}@{}>", uuid::Uuid::new_v4(), hostname)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_printable_escapes_equals_and_high_bytes() {
        let out = quoted_printable_encode("100% = café");
        assert!(out.contains("=3D"));
        assert!(out.contains("=C3=A9") || out.contains("=C3=A9\r\n"));
    }

    #[test]
    fn base64_wraps_at_76_chars() {
        let data = vec![0u8; 200];
        let out = base64_wrap(&data);
        for line in out.split("\r\n") {
            assert!(line.len() <= 76);
        }
    }

    #[test]
    fn header_phrase_ascii_passthrough() {
        assert_eq!(encode_header_phrase("John Doe"), "John Doe");
    }

    #[test]
    fn header_phrase_encodes_non_ascii() {
        let encoded = encode_header_phrase("José");
        assert!(encoded.starts_with("=?UTF-8?B?"));
    }

    // Regression: `\` must be escaped before `"` is escaped/the phrase is quoted,
    // otherwise a display name containing `,` (forcing quoting) and ending in an odd
    // run of `\` produces a quoted-string that never closes per RFC 5322 — everything
    // after on the line (including the real address) would be read as still-quoted.
    #[test]
    fn header_phrase_escapes_backslash_before_quoting() {
        // Forces quoting via the trailing comma; ends in a single backslash.
        let encoded = encode_header_phrase(r"Evil, Name\");
        assert_eq!(encoded, r#""Evil, Name\\""#);
        // The quoted-string must actually close: an even number of `\` immediately
        // before the final `"`.
        let inner = &encoded[1..encoded.len() - 1];
        let trailing_backslashes = inner.chars().rev().take_while(|&c| c == '\\').count();
        assert_eq!(trailing_backslashes % 2, 0);
    }

    #[test]
    fn header_phrase_escapes_embedded_quote_and_backslash_together() {
        let encoded = encode_header_phrase(r#"Say "hi", \friend"#);
        // Backslash escaped first, then the quote — never double-escaped.
        assert_eq!(encoded, r#""Say \"hi\", \\friend""#);
    }

    // Fixtures ported from nodemailer's test/qp/qp-test.js `#encode` cases.
    #[test]
    fn quoted_printable_matches_nodemailer_fixtures() {
        assert_eq!(
            quoted_printable_encode("abcd= ÕÄÖÜ"),
            "abcd=3D =C3=95=C3=84=C3=96=C3=9C"
        );
        assert_eq!(quoted_printable_encode("foo bar  "), "foo bar =20");
        assert_eq!(quoted_printable_encode("foo bar\t\t"), "foo bar\t=09");
        assert_eq!(quoted_printable_encode("foo \r\nbar"), "foo=20\r\nbar");
    }

    // Fixtures ported from nodemailer's test/base64/base64-test.js `#encode` cases.
    #[test]
    fn base64_matches_nodemailer_fixtures() {
        assert_eq!(base64_wrap("abcd= ÕÄÖÜ".as_bytes()), "YWJjZD0gw5XDhMOWw5w=");
        assert_eq!(base64_wrap(b"foo bar  "), "Zm9vIGJhciAg");
        assert_eq!(base64_wrap(b"foo bar\t\t"), "Zm9vIGJhcgkJ");
        assert_eq!(base64_wrap(b"foo \r\nbar"), "Zm9vIA0KYmFy");
        assert_eq!(base64_wrap(&[0x00, 0x01, 0x02, 0x20, 0x03]), "AAECIAM=");
    }

    // nodemailer: "should not emit a trailing CRLF when input is an exact multiple of lineLength".
    #[test]
    fn base64_wrap_no_trailing_crlf_on_exact_multiple() {
        let data = vec![b'A'; 114]; // 114 bytes -> 152 base64 chars = 2 * 76
        let out = base64_wrap(&data);
        assert!(!out.ends_with("\r\n"));
        assert_eq!(out.matches("\r\n").count(), 1);
    }
}
