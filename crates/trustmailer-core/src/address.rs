use serde::{Deserialize, Serialize};
use std::fmt;

/// A single mailbox (`"Name" <user@example.com>`) or an RFC 5322 group
/// (`Group: a@x.com, b@y.com;`, represented with `group: Some(members)` and an empty
/// `address`). Ported from nodemailer's `addressparser`: parsing is infallible and
/// permissive — malformed input degrades gracefully instead of erroring, matching
/// what a mail client's "To" field actually has to tolerate.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Address {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Vec<Address>>,
}

impl Address {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            address: address.into(),
            group: None,
        }
    }

    pub fn with_name(name: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            address: address.into(),
            group: None,
        }
    }

    pub fn group(name: impl Into<String>, members: Vec<Address>) -> Self {
        Self {
            name: name.into(),
            address: String::new(),
            group: Some(members),
        }
    }

    pub fn is_group(&self) -> bool {
        self.group.is_some()
    }

    /// Renders as an RFC 5322 mailbox (or group) for use inside a header value.
    /// Non-ASCII display names are wrapped in a base64 encoded-word (RFC 2047).
    pub fn to_header_value(&self) -> String {
        self.to_header_value_depth(0)
    }

    fn to_header_value_depth(&self, depth: u32) -> String {
        if let Some(members) = &self.group {
            // Guards against a directly-constructed (not parsed) `Address` graph with
            // pathological nesting depth — the parser's `MAX_NESTED_GROUP_DEPTH` only
            // protects `parse_address_list`, not `Address::group(...)` called directly
            // (e.g. from deserialized untrusted JSON), which would otherwise recurse
            // here without bound and risk a stack overflow.
            if depth > MAX_NESTED_GROUP_DEPTH {
                return String::new();
            }
            let joined = members
                .iter()
                .map(|m| m.to_header_value_depth(depth + 1))
                .collect::<Vec<_>>()
                .join(", ");
            return format!(
                "{}: {joined};",
                crate::mime::encode::encode_header_phrase(&self.name)
            );
        }
        if !self.name.is_empty() {
            format!(
                "{} <{}>",
                crate::mime::encode::encode_header_phrase(&self.name),
                self.address
            )
        } else {
            self.address.clone()
        }
    }

    /// Flattens groups into a plain list of leaf mailboxes (used for the SMTP
    /// envelope, which has no notion of groups).
    pub fn flatten(addrs: &[Address]) -> Vec<Address> {
        Self::flatten_depth(addrs, 0)
    }

    fn flatten_depth(addrs: &[Address], depth: u32) -> Vec<Address> {
        // See `to_header_value_depth` above: same depth cap, same reason.
        if depth > MAX_NESTED_GROUP_DEPTH {
            return Vec::new();
        }
        let mut out = Vec::new();
        for a in addrs {
            match &a.group {
                Some(members) => out.extend(Self::flatten_depth(members, depth + 1)),
                None => out.push(a.clone()),
            }
        }
        out
    }

    /// Like [`Address::flatten`], but yields only the address strings (no cloned
    /// `name`/`group`), avoiding pointless clones on hot paths (e.g. building the SMTP
    /// envelope) that only ever need `.address`.
    pub fn flatten_addresses(addrs: &[Address]) -> Vec<String> {
        let mut out = Vec::new();
        Self::flatten_addresses_into(addrs, 0, &mut out);
        out
    }

    fn flatten_addresses_into(addrs: &[Address], depth: u32, out: &mut Vec<String>) {
        if depth > MAX_NESTED_GROUP_DEPTH {
            return;
        }
        for a in addrs {
            match &a.group {
                Some(members) => Self::flatten_addresses_into(members, depth + 1, out),
                None => out.push(a.address.clone()),
            }
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_header_value())
    }
}

/// Anything that can be turned into a list of [`Address`]. Mirrors nodemailer accepting
/// a single string, a comma-separated string, or an array of addresses.
pub trait IntoAddresses {
    fn into_addresses(self) -> Vec<Address>;
}

impl IntoAddresses for &str {
    fn into_addresses(self) -> Vec<Address> {
        parse_address_list(self)
    }
}

impl IntoAddresses for String {
    fn into_addresses(self) -> Vec<Address> {
        parse_address_list(&self)
    }
}

impl IntoAddresses for Address {
    fn into_addresses(self) -> Vec<Address> {
        vec![self]
    }
}

impl IntoAddresses for Vec<Address> {
    fn into_addresses(self) -> Vec<Address> {
        self
    }
}

impl IntoAddresses for Vec<&str> {
    fn into_addresses(self) -> Vec<Address> {
        self.into_iter().flat_map(parse_address_list).collect()
    }
}

/// Prevents stack overflow / pathological CPU use from maliciously deeply nested
/// groups. RFC 5322 doesn't allow nested groups at all, so anything past this depth
/// is truncated rather than fully parsed.
const MAX_NESTED_GROUP_DEPTH: u32 = 50;

/// Caps the number of top-level (comma/semicolon separated) entries parsed out of a
/// single address-list string. Bounds CPU/memory on a maliciously huge `To`/`Cc`
/// value; anything past this is truncated rather than fully parsed, mirroring how
/// `MAX_NESTED_GROUP_DEPTH` truncates rather than errors on excessive nesting.
const MAX_ADDRESS_LIST_ENTRIES: usize = 4096;

/// Parses an address field (e.g. a `To:` header value) into structured addresses.
///
/// This never errors: malformed input degrades into a best-effort `Address` (possibly
/// with an empty `name` or `address`) rather than being rejected, matching how mail
/// clients actually have to handle the "From" field of the real world.
pub fn parse_address_list(input: &str) -> Vec<Address> {
    parse_address_list_depth(input, 0)
}

/// Like [`parse_address_list`], but also flattens any groups into their member
/// mailboxes (nodemailer's `{ flatten: true }` option).
pub fn parse_address_list_flatten(input: &str) -> Vec<Address> {
    Address::flatten(&parse_address_list(input))
}

fn parse_address_list_depth(input: &str, depth: u32) -> Vec<Address> {
    if depth > MAX_NESTED_GROUP_DEPTH {
        return Vec::new();
    }

    let tokens = tokenize(input);

    let mut groups: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    for token in tokens {
        if let TokenKind::Operator(op) = token.kind {
            if op == ',' || op == ';' {
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
                continue;
            }
        }
        current.push(token);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    // Bound total work on a maliciously huge address-list string: truncate rather
    // than fully parse past the cap (see `MAX_ADDRESS_LIST_ENTRIES`).
    if groups.len() > MAX_ADDRESS_LIST_ENTRIES {
        groups.truncate(MAX_ADDRESS_LIST_ENTRIES);
    }

    let parsed: Vec<Address> = groups
        .into_iter()
        .flat_map(|g| handle_address(&g, depth))
        .collect();

    merge_comma_split_names(parsed)
}

/// Merges fragments produced when an unquoted display name contains a comma:
/// "Joe Foo, PhD <joe@example.com>" tokenizes as two entries; recombine a run of
/// name-only entries followed by one with both a name and an address into a single
/// entry with the names joined by ", ".
///
/// Single forward pass building a new `Vec` (no `Vec::remove`-in-a-loop, which was
/// O(n²) on a large address list).
fn merge_comma_split_names(parsed: Vec<Address>) -> Vec<Address> {
    let mut out: Vec<Address> = Vec::with_capacity(parsed.len());
    let mut pending_names: Vec<String> = Vec::new();

    for addr in parsed {
        let is_name_only_fragment =
            addr.address.is_empty() && !addr.name.is_empty() && addr.group.is_none();
        if is_name_only_fragment {
            pending_names.push(addr.name);
            continue;
        }
        if !pending_names.is_empty() && !addr.address.is_empty() && !addr.name.is_empty() {
            pending_names.push(addr.name);
            out.push(Address {
                name: pending_names.join(", "),
                address: addr.address,
                group: addr.group,
            });
            pending_names.clear();
        } else {
            flush_pending_names(&mut pending_names, &mut out);
            out.push(addr);
        }
    }
    flush_pending_names(&mut pending_names, &mut out);

    out
}

fn flush_pending_names(pending_names: &mut Vec<String>, out: &mut Vec<Address>) {
    for name in pending_names.drain(..) {
        out.push(Address {
            name,
            address: String::new(),
            group: None,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Operator(char),
    Text,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    value: String,
    no_break: bool,
}

/// RFC 5322-ish tokenizer, ported from nodemailer's `addressparser` `Tokenizer` class.
fn tokenize(input: &str) -> Vec<Token> {
    let chars: Vec<char> = input.chars().collect();
    let mut list: Vec<Token> = Vec::new();
    let mut operator_expecting: Option<char> = None;
    let mut escaped = false;
    let mut in_domain_literal = false;
    let mut current_text: Option<usize> = None; // index into `list` of the open text token

    let operator_pair = |c: char| -> Option<char> {
        match c {
            '"' => Some('"'),
            '(' => Some(')'),
            '<' => Some('>'),
            ',' => Some('\0'), // sentinel: "no expected closer", handled below
            ':' => Some(';'),
            ';' => Some('\0'),
            _ => None,
        }
    };
    let is_list_operator = |c: char| matches!(c, ',' | ':' | ';' | '"' | '(' | '<');

    for idx in 0..chars.len() {
        let mut chr = chars[idx];
        let next_chr = chars.get(idx + 1).copied();

        if !escaped && operator_expecting.is_none() {
            if !in_domain_literal && chr == '[' {
                in_domain_literal = true;
            } else if in_domain_literal && (chr == ']' || chr == ',' || chr == ';') {
                in_domain_literal = false;
            }
        }

        if escaped {
            // fall through to text handling below
        } else if Some(chr) == operator_expecting {
            let no_break =
                matches!(next_chr, Some(c) if ![' ', '\t', '\r', '\n', ',', ';'].contains(&c));
            list.push(Token {
                kind: TokenKind::Operator(chr),
                value: chr.to_string(),
                no_break,
            });
            operator_expecting = None;
            current_text = None;
            escaped = false;
            continue;
        } else if operator_expecting.is_none() && !in_domain_literal && is_list_operator(chr) {
            list.push(Token {
                kind: TokenKind::Operator(chr),
                value: chr.to_string(),
                no_break: false,
            });
            operator_expecting = operator_pair(chr).filter(|c| *c != '\0');
            current_text = None;
            escaped = false;
            continue;
        } else if matches!(operator_expecting, Some('"')) && chr == '\\' {
            escaped = true;
            continue;
        }

        let text_idx = match current_text {
            Some(i) => i,
            None => {
                list.push(Token {
                    kind: TokenKind::Text,
                    value: String::new(),
                    no_break: false,
                });
                let i = list.len() - 1;
                current_text = Some(i);
                i
            }
        };

        if chr == '\n' || chr == '\r' {
            chr = ' ';
        }
        if (chr as u32) >= 0x21 || chr == ' ' || chr == '\t' {
            list[text_idx].value.push(chr);
        }
        escaped = false;
    }

    // Trim + drop empty text tokens (comments/operators keep their single-char value).
    list.into_iter()
        .filter_map(|mut t| {
            if t.kind == TokenKind::Text {
                t.value = t.value.trim().to_string();
                if t.value.is_empty() {
                    return None;
                }
            }
            Some(t)
        })
        .collect()
}

fn handle_address(tokens: &[Token], depth: u32) -> Vec<Address> {
    #[derive(Default)]
    struct Data {
        address: Vec<String>,
        comment: Vec<String>,
        group: Vec<String>,
        text: Vec<String>,
        text_was_quoted: Vec<bool>,
    }

    let mut data = Data::default();
    let mut state = "text";
    let mut is_group = false;
    let mut inside_quotes = false;

    for (i, token) in tokens.iter().enumerate() {
        let prev_no_break = if i > 0 { tokens[i - 1].no_break } else { false };

        if let TokenKind::Operator(op) = token.kind {
            match op {
                '<' => {
                    state = "address";
                    inside_quotes = false;
                }
                '(' => {
                    state = "comment";
                    inside_quotes = false;
                }
                ':' => {
                    state = "group";
                    is_group = true;
                    inside_quotes = false;
                }
                '"' => {
                    inside_quotes = !inside_quotes;
                    state = "text";
                }
                _ => {
                    state = "text";
                    inside_quotes = false;
                }
            }
            continue;
        }

        if token.value.is_empty() {
            continue;
        }

        let mut value = token.value.clone();
        if state == "address" {
            // An unquoted name that itself contains "<" (Apple Mail truncates
            // everything between an unexpected "<" and the real address).
            if let Some(pos) = value.find('<') {
                value = value[pos + 1..].trim_start().to_string();
            }
        }

        let bucket = match state {
            "address" => &mut data.address,
            "comment" => &mut data.comment,
            "group" => &mut data.group,
            _ => &mut data.text,
        };

        if prev_no_break && !bucket.is_empty() {
            let last = bucket.last_mut().unwrap();
            last.push_str(&value);
            if state == "text" && inside_quotes {
                if let Some(last) = data.text_was_quoted.last_mut() {
                    *last = true;
                }
            }
        } else {
            bucket.push(value);
            if state == "text" {
                data.text_was_quoted.push(inside_quotes);
            }
        }
    }

    if data.text.is_empty() && !data.comment.is_empty() {
        data.text = std::mem::take(&mut data.comment);
        // Comment tokens are never quoted; keep the parallel `text_was_quoted`
        // vector in sync so the bare-address scan below can index it safely.
        data.text_was_quoted = vec![false; data.text.len()];
    }

    if is_group {
        let name = data.text.join(" ");
        let mut group_members = Vec::new();
        if !data.group.is_empty() {
            let parsed = parse_address_list_depth(&data.group.join(","), depth + 1);
            for member in parsed {
                match member.group {
                    Some(nested) => group_members.extend(nested),
                    None => group_members.push(member),
                }
            }
        }
        return vec![Address::group(name, group_members)];
    }

    // No explicit <address>: try to detect one inside the free text.
    if data.address.is_empty() && !data.text.is_empty() {
        let mut found = false;
        for i in (0..data.text.len()).rev() {
            if !data.text_was_quoted[i] && looks_like_bare_address(&data.text[i]) {
                let addr = data.text.remove(i);
                data.text_was_quoted.remove(i);
                data.address.push(addr);
                found = true;
                break;
            }
        }
        if !found {
            for i in (0..data.text.len()).rev() {
                if data.text_was_quoted[i] {
                    continue;
                }
                if let Some((before, matched, after)) = extract_loose_address(&data.text[i]) {
                    data.address.push(matched);
                    data.text[i] = format!("{before} {after}").trim().to_string();
                    break;
                }
            }
        }
    }

    if data.text.is_empty() && !data.comment.is_empty() {
        data.text = std::mem::take(&mut data.comment);
    }

    if data.address.len() > 1 {
        let extra = data.address.split_off(1);
        data.text.extend(extra);
    }

    let text = data.text.join(" ");
    let address = data.address.join(" ");

    let mut result = Address {
        name: if !text.is_empty() {
            text.clone()
        } else {
            address.clone()
        },
        address: if !address.is_empty() { address } else { text },
        group: None,
    };

    if result.address == result.name {
        if result.address.contains('@') {
            result.name.clear();
        } else {
            result.address.clear();
        }
    }

    vec![result]
}

/// Strict match: the whole text chunk is `local@domain` with no embedded whitespace.
fn looks_like_bare_address(s: &str) -> bool {
    let Some(at) = s.find('@') else { return false };
    if at == 0 || at == s.len() - 1 {
        return false;
    }
    let (local, domain) = (&s[..at], &s[at + 1..]);
    !local.is_empty()
        && !domain.is_empty()
        && !local.chars().any(char::is_whitespace)
        && !domain.chars().any(char::is_whitespace)
        && !domain.contains('@')
}

/// Loose match: finds the first `\S+@\S+` run inside `s` and returns
/// `(before, matched, after)` with the match removed from the surrounding text.
fn extract_loose_address(s: &str) -> Option<(String, String, String)> {
    let bytes: Vec<char> = s.chars().collect();
    let mut at_pos = None;
    for (i, c) in bytes.iter().enumerate() {
        if *c == '@' {
            at_pos = Some(i);
            break;
        }
    }
    let at = at_pos?;

    let mut start = at;
    while start > 0 && !bytes[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = at;
    while end + 1 < bytes.len() && !bytes[end + 1].is_whitespace() {
        end += 1;
    }
    if start == at || end == at {
        // Need at least one char on each side of '@' with no whitespace.
        if start == at && (at == 0 || bytes[at - 1].is_whitespace()) {
            return None;
        }
    }

    let matched: String = bytes[start..=end].iter().collect();
    if !matched.contains('@') {
        return None;
    }
    let before: String = bytes[..start].iter().collect();
    let after: String = bytes[end + 1..].iter().collect();
    Some((before.trim().to_string(), matched, after.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: &str, address: &str) -> Address {
        Address::with_name(name, address)
    }
    fn bare(address: &str) -> Address {
        Address::new(address)
    }

    #[test]
    fn handles_single_address() {
        assert_eq!(
            parse_address_list("andris@tr.ee"),
            vec![bare("andris@tr.ee")]
        );
    }

    #[test]
    fn handles_multiple_addresses() {
        assert_eq!(
            parse_address_list("andris@tr.ee, andris@example.com"),
            vec![bare("andris@tr.ee"), bare("andris@example.com")]
        );
    }

    #[test]
    fn handles_unquoted_name() {
        assert_eq!(
            parse_address_list("andris <andris@tr.ee>"),
            vec![addr("andris", "andris@tr.ee")]
        );
    }

    #[test]
    fn handles_quoted_name_with_comma() {
        assert_eq!(
            parse_address_list("\"reinman, andris\" <andris@tr.ee>"),
            vec![addr("reinman, andris", "andris@tr.ee")]
        );
    }

    #[test]
    fn handles_quoted_semicolon() {
        assert_eq!(
            parse_address_list("\"reinman; andris\" <andris@tr.ee>"),
            vec![addr("reinman; andris", "andris@tr.ee")]
        );
    }

    #[test]
    fn handles_unquoted_name_unquoted_address() {
        assert_eq!(
            parse_address_list("andris andris@tr.ee"),
            vec![addr("andris", "andris@tr.ee")]
        );
    }

    #[test]
    fn keeps_colon_inside_address_literal() {
        assert_eq!(
            parse_address_list("user@[IPv6:2001:db8::1]"),
            vec![bare("user@[IPv6:2001:db8::1]")]
        );
    }

    #[test]
    fn unclosed_literal_does_not_swallow_recipients() {
        let input = "alice@example.com, bob[@example.com, carol@example.com";
        assert_eq!(
            parse_address_list(input),
            vec![
                bare("alice@example.com"),
                bare("bob[@example.com"),
                bare("carol@example.com")
            ]
        );
    }

    #[test]
    fn handles_empty_group() {
        assert_eq!(
            parse_address_list("Undisclosed:;"),
            vec![Address::group("Undisclosed", vec![])]
        );
    }

    #[test]
    fn handles_address_group() {
        let expected = Address::group(
            "Disclosed",
            vec![bare("andris@tr.ee"), bare("andris@example.com")],
        );
        assert_eq!(
            parse_address_list("Disclosed:andris@tr.ee, andris@example.com;"),
            vec![expected]
        );
    }

    #[test]
    fn handles_semicolon_as_delimiter() {
        assert_eq!(
            parse_address_list("andris@tr.ee; andris@example.com;"),
            vec![bare("andris@tr.ee"), bare("andris@example.com")]
        );
    }

    #[test]
    fn handles_name_from_comment() {
        assert_eq!(
            parse_address_list("andris@tr.ee (andris)"),
            vec![addr("andris", "andris@tr.ee")]
        );
    }

    #[test]
    fn comment_only_entry_does_not_panic() {
        // Regression: a comment-only entry moved `comment` into `text` without keeping
        // the parallel `text_was_quoted` vector in sync, so the bare-address scan
        // indexed out of bounds. The parser is infallible/never-panics by contract.
        assert_eq!(
            parse_address_list("(x)"),
            vec![Address {
                name: "x".into(),
                address: String::new(),
                group: None
            }]
        );
        // Comment-only element in the middle of a list must not crash the whole parse.
        let result = parse_address_list("a@b.com, (bar)");
        assert_eq!(result[0], addr("", "a@b.com"));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn handles_missing_address() {
        assert_eq!(
            parse_address_list("andris"),
            vec![Address {
                name: "andris".into(),
                address: String::new(),
                group: None
            }]
        );
    }

    #[test]
    fn handles_apostrophe_in_name() {
        let result = parse_address_list("O'Neill");
        assert_eq!(
            result,
            vec![Address {
                name: "O'Neill".into(),
                address: String::new(),
                group: None
            }]
        );
    }

    #[test]
    fn does_not_extract_email_from_quoted_local_part() {
        let result = parse_address_list("\"xclow3n@gmail.com x\"@internal.domain");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].address, "xclow3n@gmail.com x@internal.domain");
    }

    #[test]
    fn handles_quoted_local_part_with_attacker_domain() {
        let result = parse_address_list("\"user@attacker.com\"@legitimate.com");
        assert_eq!(result[0].address, "user@attacker.com@legitimate.com");
    }

    #[test]
    fn merges_comma_separated_display_name() {
        assert_eq!(
            parse_address_list("Joe Foo, PhD <joe@example.com>"),
            vec![addr("Joe Foo, PhD", "joe@example.com")]
        );
    }

    #[test]
    fn merges_multiple_recipients_with_commas_in_names() {
        let result = parse_address_list("Joe, PhD <j@e.com>, Jane, MD <jane@e.com>");
        assert_eq!(
            result,
            vec![addr("Joe, PhD", "j@e.com"), addr("Jane, MD", "jane@e.com")]
        );
    }

    #[test]
    fn does_not_merge_group_with_leading_name() {
        let result = parse_address_list("Title, Group: a@b.com;");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "Title");
        assert_eq!(result[1].name, "Group");
        assert!(result[1].group.is_some());
    }

    #[test]
    fn flattens_nested_groups() {
        let result = parse_address_list("Outer:Inner:deep@example.com;;");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Outer");
        let group = result[0].group.as_ref().unwrap();
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].address, "deep@example.com");
    }

    #[test]
    fn flatten_option_returns_leaf_addresses_only() {
        let result = parse_address_list_flatten("Group1:a@b.com, Group2:c@d.com;;");
        let addresses: Vec<&str> = result.iter().map(|a| a.address.as_str()).collect();
        assert!(addresses.contains(&"a@b.com"));
        assert!(addresses.contains(&"c@d.com"));
    }

    #[test]
    fn deeply_nested_groups_do_not_crash() {
        let mut input = String::new();
        for i in 0..3000 {
            input.push_str(&format!("g{i}: "));
        }
        input.push_str("user@example.com;");
        let result = parse_address_list(&input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "g0");
    }

    #[test]
    fn handles_empty_string() {
        assert_eq!(parse_address_list(""), Vec::new());
    }

    #[test]
    fn handles_whitespace_only() {
        assert_eq!(parse_address_list("   "), Vec::new());
    }

    #[test]
    fn handles_unicode_display_name() {
        let result = parse_address_list("Jüri Õunapuu <juri@example.com>");
        assert_eq!(result, vec![addr("Jüri Õunapuu", "juri@example.com")]);
    }

    #[test]
    fn handles_multiple_consecutive_delimiters() {
        let result = parse_address_list("a@example.com,,,b@example.com");
        assert_eq!(result, vec![bare("a@example.com"), bare("b@example.com")]);
    }

    #[test]
    fn handles_unclosed_angle_bracket() {
        let result = parse_address_list("Name <user@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].address, "user@example.com");
    }

    // Regression: a bare `\r` used to be silently dropped during tokenization
    // ("John\rSmith" -> "JohnSmith") instead of normalized to a space like `\n` is.
    #[test]
    fn bare_cr_normalizes_to_a_space_like_bare_lf() {
        let result = parse_address_list("John\rSmith");
        assert_eq!(
            result,
            vec![Address {
                name: "John Smith".into(),
                address: String::new(),
                group: None,
            }]
        );
        let lf_result = parse_address_list("John\nSmith");
        assert_eq!(result, lf_result);
    }

    #[test]
    fn bare_cr_in_display_name_normalizes_to_a_space() {
        let result = parse_address_list("John\rSmith <john@example.com>");
        assert_eq!(result, vec![addr("John Smith", "john@example.com")]);
    }

    // Regression: a huge address-list string used to be recombined via a `Vec::remove`
    // in a loop (O(n^2)) with no cap on total entries. This must return promptly and
    // truncate rather than hang/OOM.
    #[test]
    fn huge_address_list_is_capped_not_hung() {
        let mut input = String::new();
        for i in 0..20_000 {
            input.push_str(&format!("user{i}@example.com,"));
        }
        let result = parse_address_list(&input);
        assert!(
            result.len() <= MAX_ADDRESS_LIST_ENTRIES,
            "expected result capped at {MAX_ADDRESS_LIST_ENTRIES}, got {}",
            result.len()
        );
        assert!(!result.is_empty());
    }

    #[test]
    fn merges_long_chain_of_comma_split_name_fragments() {
        let result = parse_address_list("A, B, C, D <x@example.com>");
        assert_eq!(result, vec![addr("A, B, C, D", "x@example.com")]);
    }

    // Regression: `MAX_NESTED_GROUP_DEPTH` only guarded the text parser; a directly
    // constructed (not parsed) deeply-nested `Address` graph must not overflow the
    // stack in `flatten`/`to_header_value` either.
    #[test]
    fn flatten_does_not_overflow_on_directly_constructed_deep_nesting() {
        let mut current = Address::new("leaf@example.com");
        for i in 0..(MAX_NESTED_GROUP_DEPTH as usize * 4) {
            current = Address::group(format!("g{i}"), vec![current]);
        }
        let flat = Address::flatten(std::slice::from_ref(&current));
        // Truncated past the depth cap rather than crashing.
        assert!(flat.len() <= 1);

        // Must not panic/stack-overflow either.
        let _ = current.to_header_value();
    }
}
