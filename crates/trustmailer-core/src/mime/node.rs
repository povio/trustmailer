use super::encode::{base64_wrap, quoted_printable_encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferEncoding {
    Base64,
    QuotedPrintable,
    SevenBit,
}

#[derive(Debug)]
pub enum NodeContent {
    Multipart {
        boundary: String,
        children: Vec<MimeNode>,
    },
    Leaf {
        body: Vec<u8>,
        transfer_encoding: TransferEncoding,
    },
}

/// A node in the MIME tree. Each node owns its own headers (already RFC 2047 / boundary
/// encoded as needed) and either wraps child nodes (multipart/*) or a leaf body.
#[derive(Debug)]
pub struct MimeNode {
    pub headers: Vec<(String, String)>,
    pub content: NodeContent,
}

impl MimeNode {
    pub fn leaf(
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        transfer_encoding: TransferEncoding,
    ) -> Self {
        Self {
            headers,
            content: NodeContent::Leaf {
                body,
                transfer_encoding,
            },
        }
    }

    pub fn multipart(
        headers: Vec<(String, String)>,
        boundary: String,
        children: Vec<MimeNode>,
    ) -> Self {
        Self {
            headers,
            content: NodeContent::Multipart { boundary, children },
        }
    }

    /// Serializes this node (and its subtree) into a full RFC 5322 / MIME byte stream.
    pub fn render(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut Vec<u8>) {
        for (name, value) in &self.headers {
            write_header_line(out, name, value);
        }

        match &self.content {
            NodeContent::Leaf {
                body,
                transfer_encoding,
            } => {
                out.extend_from_slice(b"\r\n");
                match transfer_encoding {
                    TransferEncoding::Base64 => {
                        out.extend_from_slice(base64_wrap(body).as_bytes());
                    }
                    TransferEncoding::QuotedPrintable => {
                        let text = String::from_utf8_lossy(body);
                        out.extend_from_slice(quoted_printable_encode(&text).as_bytes());
                    }
                    TransferEncoding::SevenBit => {
                        out.extend_from_slice(body);
                    }
                }
                out.extend_from_slice(b"\r\n");
            }
            NodeContent::Multipart { boundary, children } => {
                out.extend_from_slice(b"\r\n");
                for child in children {
                    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                    child.write(out);
                }
                out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            }
        }
    }
}

fn write_header_line(out: &mut Vec<u8>, name: &str, value: &str) {
    // Security: a header *value* that itself contains a CRLF (or bare CR/LF) would let
    // an attacker-controlled string (subject, a List-* comment, a custom header, ...)
    // terminate this header early and inject arbitrary follow-on header lines. Collapse
    // any such sequence to a single space, matching nodemailer's `mime-node` sanitizer.
    let sanitized_name = strip_crlf(name);
    let sanitized_value = strip_crlf(value);
    let line = format!("{sanitized_name}: {sanitized_value}");
    out.extend_from_slice(fold_header(&line).as_bytes());
    out.extend_from_slice(b"\r\n");
}

fn strip_crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(' ');
            }
            '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

/// Folds a header line at commas so no line exceeds ~76 columns, per RFC 5322 §2.2.3.
/// Continuation lines are indented with a single space (folding whitespace).
///
/// Splits only on commas *outside* a quoted-string (mirroring the `inside_quotes`
/// tracking the address tokenizer uses) — otherwise a comma inside a quoted display
/// name (e.g. `"Foo, Bar" <a@b.com>`) could be folded on, injecting whitespace into
/// quoted content.
fn fold_header(line: &str) -> String {
    if line.len() <= 76 {
        return line.to_string();
    }
    let parts = split_outside_quotes(line, ',');
    let mut out = String::new();
    let mut col = 0usize;
    for (i, part) in parts.iter().enumerate() {
        let piece = if i == 0 {
            part.clone()
        } else {
            format!(",{part}")
        };
        if col > 0 && col + piece.len() > 76 {
            out.push_str("\r\n ");
            col = 1;
        }
        out.push_str(&piece);
        col += piece.len();
    }
    out
}

/// Splits `s` on `sep` characters that occur outside a `"`-quoted span (with `\`
/// treated as an escape inside quotes, so `\"` doesn't end the quoted span).
fn split_outside_quotes(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut inside_quotes = false;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if inside_quotes => {
                escaped = true;
                current.push(c);
            }
            '"' => {
                inside_quotes = !inside_quotes;
                current.push(c);
            }
            c if c == sep && !inside_quotes => {
                parts.push(std::mem::take(&mut current));
            }
            c => current.push(c),
        }
    }
    parts.push(current);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_outside_quotes_ignores_commas_inside_quoted_spans() {
        let parts = split_outside_quotes(r#""Foo, Bar" <a@b.com>, "Baz, Qux" <c@d.com>"#, ',');
        assert_eq!(
            parts,
            vec![
                r#""Foo, Bar" <a@b.com>"#.to_string(),
                r#" "Baz, Qux" <c@d.com>"#.to_string(),
            ]
        );
    }

    // Regression: `fold_header` used to split on every literal `,` with no quoted-string
    // awareness, which could fold a header line in the middle of a quoted display name.
    #[test]
    fn fold_header_does_not_split_inside_a_quoted_display_name() {
        // A single long address list whose only commas are inside quoted display names;
        // long enough (>76 cols) to force folding.
        let name_a = "A".repeat(40);
        let name_b = "B".repeat(40);
        let line =
            format!("To: \"{name_a}, PhD\" <a@example.com>, \"{name_b}, MD\" <b@example.com>");
        assert!(line.len() > 76);
        let folded = fold_header(&line);
        // Every quoted span must survive intact (comma-space still directly inside it),
        // i.e. folding never inserted "\r\n " in the middle of `"..., ..."`.
        assert!(folded.contains(&format!("\"{name_a}, PhD\"")));
        assert!(folded.contains(&format!("\"{name_b}, MD\"")));
    }

    #[test]
    fn fold_header_still_folds_a_long_line_at_unquoted_commas() {
        let line = format!(
            "To: {}, {}",
            "a@example.com".repeat(4),
            "b@example.com".repeat(4)
        );
        let folded = fold_header(&line);
        assert!(folded.contains("\r\n "));
    }
}
