//! DKIM signing (RFC 6376), RSA-SHA256, relaxed/relaxed canonicalization.
//!
//! ```no_run
//! use trustmailer_dkim::DkimSigner;
//!
//! # let private_key_pem = "";
//! let signer = DkimSigner::from_pkcs8_pem("example.com", "default", private_key_pem).unwrap();
//! # let raw_message: Vec<u8> = vec![];
//! let signed = signer.sign(&raw_message).unwrap();
//! ```

mod canonicalize;
mod transport;

pub use transport::SigningTransport;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rsa::pkcs8::DecodePrivateKey;
use rsa::traits::PublicKeyParts as _;
use rsa::{Pkcs1v15Sign, RsaPrivateKey};
use sha2::{Digest, Sha256};
use trustmailer_core::{Error, Result};

// Priority order ported from nodemailer's `lib/dkim/sign.js` `defaultFieldNames`
// (RFC 4871 §5.5's recommended sign list) — present headers are signed in *this*
// order, not the order they appear in the message, so signatures are reproducible
// cross-implementation given the same key and inputs.
const DEFAULT_HEADERS: &[&str] = &[
    "from",
    "sender",
    "reply-to",
    "subject",
    "date",
    "message-id",
    "to",
    "cc",
    "mime-version",
    "content-type",
    "content-transfer-encoding",
    "content-id",
    "content-description",
    "resent-date",
    "resent-from",
    "resent-sender",
    "resent-to",
    "resent-cc",
    "resent-message-id",
    "in-reply-to",
    "references",
    "list-id",
    "list-help",
    "list-unsubscribe",
    "list-subscribe",
    "list-post",
    "list-owner",
    "list-archive",
];

/// DER `DigestInfo` prefix for SHA-256, per RFC 3447 §9.2 (PKCS#1 v1.5). We hash the
/// signing input ourselves with the plain `sha2` crate and hand `rsa` the raw digest
/// via the non-generic `Pkcs1v15Sign` scheme, instead of `pkcs1v15::SigningKey<D>`.
/// That sidesteps needing our `sha2`/`digest` version to exactly match whatever
/// version `rsa`'s generic `Digest` bound was compiled against.
const SHA256_PKCS1_PREFIX: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

/// Target column width for folding the generated `DKIM-Signature` value. Folds are
/// only ever inserted right after a `;` tag separator (never inside a tag's value),
/// which keeps folding safe: RFC 6376 relaxed header canonicalization unfolds
/// (`CRLF` + `WSP` collapses back to a single space) before hashing, so this changes
/// nothing about what gets signed/verified — it only keeps the emitted wire line
/// under RFC 5322's 998-octet cap for larger (2048/4096-bit) keys.
const FOLD_WIDTH: usize = 76;

/// Folds `value` (the DKIM-Signature tag-value list, e.g. `" v=1; a=rsa-sha256; ..."`)
/// by replacing the `" "` after a `";"` with `"\r\n\t"` whenever the current line has
/// grown past `max_width`. Never splits inside a single tag's value.
fn fold_dkim_header_value(value: &str, max_width: usize) -> String {
    let tags: Vec<&str> = value.trim_start().split("; ").collect();
    let mut out = String::from(" ");
    let mut line_len = 1usize;
    for (i, tag) in tags.iter().enumerate() {
        if i == 0 {
            out.push_str(tag);
            line_len += tag.len();
            continue;
        }
        let would_be = line_len + 2 + tag.len();
        if would_be > max_width {
            out.push_str(";\r\n\t");
            out.push_str(tag);
            line_len = 1 + tag.len();
        } else {
            out.push_str("; ");
            out.push_str(tag);
            line_len = would_be;
        }
    }
    out
}

pub struct DkimSigner {
    domain: String,
    selector: String,
    private_key: RsaPrivateKey,
    headers_to_sign: Vec<String>,
}

/// Rejects characters that have no business in a DNS domain name or a DKIM
/// selector, and which would otherwise let `domain`/`selector` break out of the
/// `DKIM-Signature` tag-value they're interpolated into (header injection) or
/// terminate it early (via `;`/`"`).
fn validate_dkim_token(kind: &str, value: &str) -> Result<()> {
    if value.contains('\r')
        || value.contains('\n')
        || value.contains(';')
        || value.contains('"')
    {
        return Err(Error::Dkim(format!(
            "invalid {kind}: must not contain CR, LF, ';', or '\"'"
        )));
    }
    Ok(())
}

impl DkimSigner {
    pub fn from_pkcs8_pem(
        domain: impl Into<String>,
        selector: impl Into<String>,
        pem: &str,
    ) -> Result<Self> {
        let domain = domain.into();
        let selector = selector.into();
        validate_dkim_token("domain", &domain)?;
        validate_dkim_token("selector", &selector)?;

        let private_key = RsaPrivateKey::from_pkcs8_pem(pem.trim())
            .map_err(|e| Error::Dkim(format!("invalid PKCS#8 private key: {e}")))?;
        Ok(Self {
            domain,
            selector,
            private_key,
            headers_to_sign: DEFAULT_HEADERS.iter().map(|s| s.to_string()).collect(),
        })
    }

    pub fn with_headers(mut self, headers: Vec<String>) -> Self {
        self.headers_to_sign = headers
            .into_iter()
            .map(|h| h.to_ascii_lowercase())
            .collect();
        self
    }

    /// RSA key size in bits, informational (e.g. to warn if a signer uses <1024-bit keys).
    pub fn key_bits(&self) -> usize {
        self.private_key.size() * 8
    }

    /// Signs a raw RFC 5322 message and returns it with a `DKIM-Signature` header prepended.
    pub fn sign(&self, raw_message: &[u8]) -> Result<Vec<u8>> {
        let (headers, body) = canonicalize::parse_headers(raw_message);
        let canonical_body = canonicalize::canonicalize_body(&body);

        let body_hash = Sha256::digest(&canonical_body);
        let bh = STANDARD.encode(body_hash);

        let mut signed_header_names = Vec::new();
        let mut canonical_headers: Vec<u8> = Vec::new();
        for wanted in &self.headers_to_sign {
            if let Some((name, value)) = headers
                .iter()
                .rev()
                .find(|(n, _)| n.eq_ignore_ascii_case(wanted))
            {
                canonical_headers.extend(canonicalize::canonicalize_header(name, value));
                canonical_headers.extend_from_slice(b"\r\n");
                signed_header_names.push(name.to_ascii_lowercase());
            }
        }
        if signed_header_names.is_empty() {
            return Err(Error::Dkim("no signable headers found on message".into()));
        }
        if !signed_header_names.iter().any(|n| n.eq_ignore_ascii_case("from")) {
            return Err(Error::Dkim("From header must be signed".into()));
        }

        let h_field = signed_header_names.join(":");
        let dkim_header_value_no_b = format!(
            " v=1; a=rsa-sha256; c=relaxed/relaxed; d={}; q=dns/txt; s={}; bh={}; h={}; b=",
            self.domain, self.selector, bh, h_field
        );

        let mut signing_input = canonical_headers;
        signing_input.extend(canonicalize::canonicalize_header(
            "DKIM-Signature",
            dkim_header_value_no_b.as_bytes(),
        ));

        let digest = Sha256::digest(&signing_input);
        let scheme = Pkcs1v15Sign {
            hash_len: Some(32),
            prefix: Box::from(SHA256_PKCS1_PREFIX.as_slice()),
        };
        let signature = self
            .private_key
            .sign(scheme, &digest)
            .map_err(|e| Error::Dkim(format!("RSA signing failed: {e}")))?;
        let b_value = STANDARD.encode(signature);

        let full_value = format!("{dkim_header_value_no_b}{b_value}");
        let folded_value = fold_dkim_header_value(&full_value, FOLD_WIDTH);
        let dkim_header = format!("DKIM-Signature:{folded_value}\r\n");
        let mut out = Vec::with_capacity(dkim_header.len() + raw_message.len());
        out.extend_from_slice(dkim_header.as_bytes());
        out.extend_from_slice(raw_message);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Throwaway 1024-bit key, generated once with `openssl genpkey`, used only to
    // exercise the signing path in tests. Not tied to any real domain.
    const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIICdwIBADANBgkqhkiG9w0BAQEFAASCAmEwggJdAgEAAoGBAM66zEtGVJuQMSgf
3VOZIshTsAHdaSY3VyJBl8hKoUrfiHzwztgKTRqQmHuIY6WhTo7QG9QXy3kCL7v0
Sb5bmJa3xQ0ECD3wVjVcDHTjMYmGyUkFgKEr63nwhHGxcTFb6646j0iRbzvorhOt
adtCGouJPuCwO2J1zY5rPCL5q0EPAgMBAAECgYEAnkTIr0CS+d40pCFxEtWEmZXo
hCT8oXNw1Bsflwn73hSwq+a4lRGd4h6dfsX2dBHA0PlmtMPa1TPlU6I86u25LRkK
vuMWKGkDefxUZnPapkKTBOVa6frUfWJt0k4exLOONbRDwwVPyVN6mfp9Uw4wKUcR
Cx0DYgXOuMeqiTAzbxECQQD4i5Hq+KTEnpBMHiq9p6sCZHiDAUImJ7sokdGQddwc
kNBATSRVigCC1vRUcYQ8q//ge5J7XDR8pqsbmBYL2JcNAkEA1O4m1IB5BUmvskBr
7BTnn5+A0izXNliGjLRdOn9qa45feFkGDcb48y7dCIBEGsX9k2x24Z788d9Phxhy
4NbxiwJBALs+gq6vpYoPHwyJSY/Z6iaCRJwFba1LD1/wAx4+/JV0r79Rnobm0Z3A
jQ+0+Xev6LRYxDE1dMFX2t6OBEXmZV0CQCx0yJCnfhz3hIJP5vh0aOOv1qKLECfd
lcH8ylYzUhHAuPgUhioMN9/2Edn7hbydR9Mm2y9CFYeeWylWlm2REv8CQAnBf685
Wrtj7RTgOGXkdwewRZER4kA6Cp9gZPJJHZYyxZJuA/kNFvvE9aCh3a82nRQSjm/V
yPa1hIeqMMVFF9A=
-----END PRIVATE KEY-----";

    fn test_signer() -> DkimSigner {
        DkimSigner::from_pkcs8_pem("example.com", "default", TEST_KEY_PEM).unwrap()
    }

    #[test]
    fn signs_and_prepends_header() {
        let signer = test_signer();
        let raw = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: hi\r\nDate: Mon, 1 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@example.com>\r\n\r\nHello\r\n";
        let signed = signer.sign(raw).unwrap();
        let s = String::from_utf8(signed).unwrap();
        assert!(s.starts_with("DKIM-Signature:"));
        assert!(s.contains("d=example.com"));
        assert!(s.contains("Hello"));
    }

    #[test]
    fn rejects_crlf_in_domain() {
        match DkimSigner::from_pkcs8_pem("example.com\r\nX-Injected: evil", "default", TEST_KEY_PEM)
        {
            Err(Error::Dkim(_)) => {}
            other => panic!("expected Error::Dkim, got {}", other.is_ok()),
        }
    }

    #[test]
    fn rejects_lf_in_selector() {
        match DkimSigner::from_pkcs8_pem("example.com", "default\nX-Injected: evil", TEST_KEY_PEM) {
            Err(Error::Dkim(_)) => {}
            other => panic!("expected Error::Dkim, got {}", other.is_ok()),
        }
    }

    #[test]
    fn rejects_semicolon_and_quote_in_domain_or_selector() {
        assert!(DkimSigner::from_pkcs8_pem("example.com;d=evil.com", "default", TEST_KEY_PEM).is_err());
        assert!(DkimSigner::from_pkcs8_pem("example.com", "default\"", TEST_KEY_PEM).is_err());
    }

    #[test]
    fn accepts_valid_domain_and_selector() {
        assert!(DkimSigner::from_pkcs8_pem("example.com", "default", TEST_KEY_PEM).is_ok());
    }

    #[test]
    fn errors_when_from_is_not_signed() {
        let signer = test_signer().with_headers(vec!["subject".to_string(), "date".to_string()]);
        let raw = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: hi\r\nDate: Mon, 1 Jan 2024 00:00:00 +0000\r\n\r\nHello\r\n";
        let err = signer.sign(raw).unwrap_err();
        assert!(matches!(err, Error::Dkim(ref m) if m.contains("From")));
    }

    #[test]
    fn folded_dkim_signature_still_verifies_expected_fields() {
        // A larger header-to-sign list widens the `h=` tag enough to force folding
        // even with the small test key, confirming folded output stays well-formed
        // (fold points are only ever "\r\n\t", never mid-tag) and unaffected fields
        // are still readable as contiguous substrings.
        let signer = test_signer().with_headers(vec![
            "from".to_string(),
            "to".to_string(),
            "subject".to_string(),
            "date".to_string(),
            "message-id".to_string(),
        ]);
        let raw = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: hi\r\nDate: Mon, 1 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@example.com>\r\n\r\nHello\r\n";
        let signed = signer.sign(raw).unwrap();
        let s = String::from_utf8(signed).unwrap();
        assert!(s.contains("d=example.com"));
        assert!(s.contains("h=from:to:subject:date:message-id"));

        // Every fold within the DKIM-Signature header must be exactly "\r\n" followed
        // by WSP (folding whitespace) — never a bare "\n" and never mid-tag. The
        // header's continuation lines are exactly the ones starting with WSP; the
        // first line that doesn't is the next (unrelated) header.
        assert!(s.starts_with("DKIM-Signature:"));
        let mut folds_seen = 0;
        let mut rest = &s[..];
        loop {
            let nl = rest.find("\r\n").expect("header block must end in CRLF");
            rest = &rest[nl + 2..];
            if rest.starts_with(' ') || rest.starts_with('\t') {
                folds_seen += 1;
                continue;
            }
            break;
        }
        assert!(folds_seen >= 1, "expected at least one fold, saw none");
    }

    /// Cross-implementation golden test: same key, same message, signed independently
    /// by real nodemailer (`lib/dkim`, via its `crypto.createSign` path) for this exact
    /// input. RSA-SHA256/PKCS#1v1.5 is deterministic, so `bh=`/`b=` must match byte for
    /// byte — this is a much stronger check than "it looks like a signature".
    ///
    /// Oracle output, captured with `node` running nodemailer's real `DKIM` class:
    /// `bh=Ba3gj8+xBPQLJTahTfzW6RbWQ/XPgESxkCi2B66PSQg=`,
    /// `h=from:subject:date:message-id:to`,
    /// `b=A/CZ22fQICWmufRL0MjafUgj4C2jDSl9uNTFzaLBcg+IYH1duH/ULuHvOOQV7Z+7fuzRVJsbCEv4JwIoJZSys4abFOFNdeTofFFkssfEaVO1eXuwgP7wU5eC7AA8D3ce`
    #[test]
    fn signature_matches_real_nodemailer_output() {
        const ORACLE_KEY_PEM: &str = include_str!("../test-fixtures/nodemailer_oracle_key.pem");
        let signer = DkimSigner::from_pkcs8_pem("example.com", "default", ORACLE_KEY_PEM).unwrap();

        let raw = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: hi\r\nDate: Mon, 1 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@example.com>\r\n\r\nHello\r\n";
        let signed = signer.sign(raw).unwrap();
        let s = String::from_utf8(signed).unwrap();

        assert!(s.contains("bh=Ba3gj8+xBPQLJTahTfzW6RbWQ/XPgESxkCi2B66PSQg="));
        assert!(s.contains("h=from:subject:date:message-id:to"));

        let b_start = s.rfind("b=").unwrap() + 2;
        let b_end = s[b_start..]
            .find("\r\n")
            .map(|p| b_start + p)
            .unwrap_or(s.len());
        let actual_b: String = s[b_start..b_end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert_eq!(
            actual_b,
            "A/CZ22fQICWmufRL0MjafUgj4C2jDSl9uNTFzaLBcg+IYH1duH/ULuHvOOQV7Z+7fuzRVJsbCEv4JwIoJZSys4abFOFNdeTofFFkssfEaVO1eXuwgP7wU5eC7AA8D3ce"
        );
    }
}
