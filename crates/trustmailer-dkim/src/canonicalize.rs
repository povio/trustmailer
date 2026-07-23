/// Splits a raw RFC 5322 message into `(headers, body)`, unfolding continuation
/// lines (a line starting with SP/TAB is a continuation of the previous header).
///
/// Operates on raw bytes throughout (never decodes through `String`/`char`) so
/// non-UTF-8 header/body content (legacy 8bit charsets, binary content-transfer
/// encodings) survives untouched — the DKIM body hash must be computed over the
/// exact wire bytes, not a UTF-8-lossy approximation of them.
pub fn parse_headers(raw: &[u8]) -> (Vec<(String, Vec<u8>)>, Vec<u8>) {
    let lines = split_crlf_lines(raw);
    let mut headers: Vec<(String, Vec<u8>)> = Vec::new();
    let mut current: Option<(String, Vec<u8>)> = None;

    for line in lines {
        if line.is_empty() {
            break;
        }
        if line.starts_with(b" ") || line.starts_with(b"\t") {
            if let Some((_, value)) = current.as_mut() {
                value.push(b' ');
                value.extend_from_slice(trim_wsp(line));
                continue;
            }
        }
        if let Some(pair) = current.take() {
            headers.push(pair);
        }
        if let Some(colon) = line.iter().position(|&b| b == b':') {
            // Trim ASCII space/tab around the header name (e.g. `"Subject : value"`)
            // so it isn't silently dropped by a case-insensitive match later.
            let name = String::from_utf8_lossy(trim_wsp(&line[..colon])).into_owned();
            let value = line[colon + 1..].to_vec();
            current = Some((name, value));
        }
    }
    if let Some(pair) = current {
        headers.push(pair);
    }

    // Reconstruct the byte offset of the body: find the first "\r\n\r\n".
    let header_block_len = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(raw.len());
    let body = raw[header_block_len.min(raw.len())..].to_vec();

    (headers, body)
}

/// Splits `data` on `"\r\n"`, matching `str::split("\r\n")` semantics but on raw
/// bytes (no UTF-8 validity requirement).
fn split_crlf_lines(mut data: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    loop {
        match data.windows(2).position(|w| w == b"\r\n") {
            Some(pos) => {
                lines.push(&data[..pos]);
                data = &data[pos + 2..];
            }
            None => {
                lines.push(data);
                break;
            }
        }
    }
    lines
}

/// Trims RFC 6376 `WSP` (`SP`/`HTAB` only — *not* the full Unicode whitespace
/// class) from both ends of a byte slice.
fn trim_wsp(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&b| b != b' ' && b != b'\t')
        .map(|p| p + 1)
        .unwrap_or(0);
    if start >= end {
        &[]
    } else {
        &s[start..end]
    }
}

/// Trims RFC 6376 `WSP` from the end of a byte slice only.
fn trim_end_wsp(s: &[u8]) -> &[u8] {
    let end = s
        .iter()
        .rposition(|&b| b != b' ' && b != b'\t')
        .map(|p| p + 1)
        .unwrap_or(0);
    &s[..end]
}

/// Relaxed header canonicalization per RFC 6376 §3.4.2.
pub fn canonicalize_header(name: &str, value: &[u8]) -> Vec<u8> {
    let lower_name = name.to_ascii_lowercase();
    let collapsed = collapse_whitespace_bytes(trim_wsp(value));
    let mut out = Vec::with_capacity(lower_name.len() + 1 + collapsed.len());
    out.extend_from_slice(lower_name.as_bytes());
    out.push(b':');
    out.extend_from_slice(&collapsed);
    out
}

/// Collapses runs of RFC 6376 `WSP` (`SP`/`HTAB`) into a single space, leaving
/// every other byte (including non-UTF-8 bytes) untouched.
fn collapse_whitespace_bytes(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut last_was_space = false;
    for &b in s {
        if b == b' ' || b == b'\t' {
            if !last_was_space {
                out.push(b' ');
            }
            last_was_space = true;
        } else {
            out.push(b);
            last_was_space = false;
        }
    }
    out
}

/// Relaxed body canonicalization per RFC 6376 §3.4.4: collapse intra-line
/// whitespace, strip trailing whitespace per line, and drop trailing blank lines
/// (a non-empty body always ends with exactly one CRLF).
///
/// Operates on raw bytes (no `String`/`char` decoding step) in a single pass over
/// a `\n`-normalized copy of `body`, with no `Vec<String>` intermediate — the only
/// allocations are the normalized-CRLF buffer and the final output buffer.
pub fn canonicalize_body(body: &[u8]) -> Vec<u8> {
    // Normalize CRLF -> LF, one pass, byte-level (never decodes through UTF-8, so
    // non-UTF-8 body bytes can't be corrupted into U+FFFD before hashing).
    let mut normalized = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] == b'\r' && body.get(i + 1) == Some(&b'\n') {
            normalized.push(b'\n');
            i += 2;
        } else {
            normalized.push(body[i]);
            i += 1;
        }
    }

    let lines: Vec<&[u8]> = normalized.split(|&b| b == b'\n').collect();

    // Trailing blank lines are dropped. A line is blank once trailing WSP is
    // stripped, which is exactly what a "whitespace-only" line collapses to.
    let mut last = lines.len();
    while last > 0 && trim_end_wsp(lines[last - 1]).is_empty() {
        last -= 1;
    }
    if last == 0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(normalized.len() + 2);
    for (idx, line) in lines[..last].iter().enumerate() {
        if idx > 0 {
            out.extend_from_slice(b"\r\n");
        }
        out.extend(collapse_whitespace_bytes(line));
        // Collapsing can leave at most one trailing WSP byte; strip it.
        while matches!(out.last(), Some(&b' ') | Some(&b'\t')) {
            out.pop();
        }
    }
    out.extend_from_slice(b"\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_header_relaxed() {
        assert_eq!(
            canonicalize_header("Subject", b"  Hello   World  "),
            b"subject:Hello World".to_vec()
        );
    }

    #[test]
    fn canonicalizes_body_drops_trailing_blank_lines() {
        let body = b"Hello  \r\nWorld\r\n\r\n\r\n";
        let out = canonicalize_body(body);
        assert_eq!(out, b"Hello\r\nWorld\r\n");
    }

    #[test]
    fn empty_body_canonicalizes_to_empty() {
        assert_eq!(canonicalize_body(b""), Vec::<u8>::new());
    }

    /// Non-UTF-8 body bytes (e.g. a legacy `iso-8859-1`/`windows-1252` 8bit body)
    /// must survive canonicalization untouched — no lossy UTF-8 decode step should
    /// ever replace them with U+FFFD before hashing.
    #[test]
    fn preserves_non_utf8_body_bytes() {
        // 0xE9 alone is invalid UTF-8 (would decode as U+FFFD under `from_utf8_lossy`).
        let body: &[u8] = b"Caf\xE9 au lait\r\n";
        let out = canonicalize_body(body);
        assert_eq!(out, body);
        assert!(!out.contains(&0xEF)); // no accidental U+FFFD (EF BF BD) encoding
    }

    /// Header names with surrounding whitespace before the colon must still match
    /// during case-insensitive signing lookups instead of being silently dropped.
    #[test]
    fn parses_header_name_with_surrounding_whitespace() {
        let raw = b"Subject : hi\r\n\r\nbody\r\n";
        let (headers, _) = parse_headers(raw);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0, "Subject");
    }

    /// Cross-implementation golden test ported from nodemailer's
    /// `test/dkim/relaxed-body-test.js`: same fixture message, same expected relaxed
    /// body hash (`D2H5TEwtUgM2u8Ew0gG6vnt/Na6L+Zep7apmSmfy8IQ=`), computed independently
    /// by nodemailer's `lib/dkim/relaxed-body` stream implementation.
    #[test]
    fn relaxed_body_hash_matches_nodemailer_fixture() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use sha2::{Digest, Sha256};

        let raw = include_bytes!("../test-fixtures/message1.eml");
        let text = String::from_utf8_lossy(raw)
            .replace("\r\n", "\n")
            .replace('\n', "\r\n");
        let (_headers, body) = parse_headers(text.as_bytes());

        let canonical = canonicalize_body(&body);
        let hash = STANDARD.encode(Sha256::digest(&canonical));

        assert_eq!(hash, "D2H5TEwtUgM2u8Ew0gG6vnt/Na6L+Zep7apmSmfy8IQ=");
    }
}
