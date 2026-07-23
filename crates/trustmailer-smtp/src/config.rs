use crate::proxy::ProxyConfig;
use std::time::Duration;
use trustmailer_core::{Error, Result};
use trustmailer_smtp_protocol::{strip_crlf, Auth};

/// Default cap on connect + TLS/STARTTLS handshake time.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Default cap on any single SMTP command's write+read round trip.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// Connect with implicit TLS (port 465 style). When `false` and `require_tls` is
    /// `true`, STARTTLS is negotiated instead (port 587 style).
    pub secure: bool,
    pub require_tls: bool,
    pub auth: Option<Auth>,
    /// Name sent in the `EHLO` command; defaults to `localhost`.
    pub ehlo_name: String,
    /// Max concurrent connections when used through [`crate::SmtpPool`].
    pub pool_size: usize,
    /// Tunnel the connection through a SOCKS5 proxy.
    pub proxy: Option<ProxyConfig>,
    /// Max time to wait for the initial TCP connect and the TLS/STARTTLS handshake.
    pub connect_timeout: Duration,
    /// Max time to wait for any single SMTP command's response (including reading a
    /// multi-line response in full).
    pub command_timeout: Duration,
    /// Allow `AUTH` to be attempted over a connection that isn't `secure` and didn't
    /// complete STARTTLS. Defaults to `false` — sending credentials in cleartext is a
    /// realistic misconfiguration (e.g. an internal relay on a custom port), and
    /// callers who genuinely want that must opt in explicitly.
    pub allow_insecure_auth: bool,
    /// Decline the opportunistic STARTTLS upgrade even when the server advertises it.
    /// Defaults to `false` (upgrade whenever offered). Set this for an internal relay
    /// that advertises STARTTLS but presents an untrusted/self-signed certificate the
    /// hardcoded `webpki-roots` trust store will reject — without this escape the
    /// forced handshake fails and the connection becomes unusable. Ignored (upgrade
    /// still forced) when `require_tls` is set, since that explicitly demands TLS.
    pub disable_starttls: bool,
}

impl SmtpConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            secure: port == 465,
            require_tls: port == 587,
            auth: None,
            ehlo_name: "localhost".to_string(),
            pool_size: 5,
            proxy: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            allow_insecure_auth: false,
            disable_starttls: false,
        }
    }

    /// Looks up a well-known service (e.g. `"gmail"`) via [`trustmailer_core::well_known`].
    pub fn for_service(service: &str) -> Option<Self> {
        trustmailer_core::well_known::lookup_service(service)
            .map(|s| Self::new(s.host, s.port).with_secure(s.secure))
    }

    pub fn with_secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self.require_tls = !secure;
        self
    }

    pub fn with_auth(mut self, auth: Auth) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Sets the name sent in the `EHLO` command. Any CR/LF is stripped (not just
    /// escaped) — this value is written directly into an SMTP command, so a caller
    /// passing untrusted input here must not be able to inject a second command.
    pub fn with_ehlo_name(mut self, name: impl Into<String>) -> Self {
        self.ehlo_name = strip_crlf(&name.into());
        self
    }

    pub fn with_pool_size(mut self, size: usize) -> Self {
        self.pool_size = size;
        self
    }

    pub fn with_proxy(mut self, proxy: ProxyConfig) -> Self {
        self.proxy = Some(proxy);
        self
    }

    /// Sets the max time to wait for the initial TCP connect and the TLS/STARTTLS
    /// handshake. Defaults to [`DEFAULT_CONNECT_TIMEOUT`].
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the max time to wait for any single SMTP command's response. Defaults to
    /// [`DEFAULT_COMMAND_TIMEOUT`].
    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Allows `AUTH` to be sent over a connection that isn't `secure` and didn't
    /// complete STARTTLS. Off by default — see [`SmtpConfig::allow_insecure_auth`].
    pub fn with_allow_insecure_auth(mut self, allow: bool) -> Self {
        self.allow_insecure_auth = allow;
        self
    }

    /// Declines the opportunistic STARTTLS upgrade even when advertised. Off by default
    /// — see [`SmtpConfig::disable_starttls`]. Has no effect when `require_tls` is set.
    pub fn with_disable_starttls(mut self, disable: bool) -> Self {
        self.disable_starttls = disable;
        self
    }

    /// Parses a connection URL — `smtp://[user[:pass]@]host[:port]` (plaintext) or
    /// `smtps://...` (implicit TLS) — mirroring nodemailer's
    /// `nodemailer.createTransport("smtps://user:pass@smtp.gmail.com")` shorthand.
    pub fn from_url(url: &str) -> Result<Self> {
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| Error::Transport(format!("not a connection URL: {url}")))?;
        let secure = match scheme {
            "smtp" => false,
            "smtps" => true,
            other => {
                return Err(Error::Transport(format!(
                    "unsupported connection URL scheme: {other}"
                )))
            }
        };

        // Drop any path/query — only the authority (userinfo@host:port) is used.
        let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);

        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, authority),
        };

        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().ok()),
            None => (hostport, None),
        };
        if host.is_empty() {
            return Err(Error::Transport(format!(
                "connection URL is missing a host: {url}"
            )));
        }

        let port = port.unwrap_or(if secure { 465 } else { 587 });
        let mut config = Self::new(host, port).with_secure(secure);

        if let Some(userinfo) = userinfo {
            let (user, pass) = match userinfo.split_once(':') {
                Some((u, p)) => (percent_decode(u), percent_decode(p)),
                None => (percent_decode(userinfo), String::new()),
            };
            config = config.with_auth(Auth::Plain {
                username: user,
                password: pass,
            });
        }

        Ok(config)
    }
}

/// Percent-decodes `%XX` escapes by byte, never by `&str` byte-range slicing: a `%`
/// immediately followed by non-ASCII (e.g. multi-byte UTF-8) bytes used to make
/// `s[i+1..i+3]` panic when that range split a multi-byte character instead of
/// landing on a char boundary. Working purely over `.as_bytes()` and only treating a
/// `%` as an escape once both following bytes are confirmed ASCII hex digits avoids
/// that entirely.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hi = (bytes[i + 1] as char).to_digit(16).unwrap();
            let lo = (bytes[i + 2] as char).to_digit(16).unwrap();
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Hand-written rather than derived: `auth`/`proxy` may carry plaintext credentials
/// nested inside them. `Auth`/`ProxyConfig` already redact their own secret fields in
/// their `Debug` impls, but this makes the guarantee explicit at this level too rather
/// than relying on that alone.
impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("secure", &self.secure)
            .field("require_tls", &self.require_tls)
            .field("allow_insecure_auth", &self.allow_insecure_auth)
            .field("disable_starttls", &self.disable_starttls)
            .field("auth", &self.auth)
            .field("ehlo_name", &self.ehlo_name)
            .field("pool_size", &self.pool_size)
            .field("proxy", &self.proxy)
            .field("connect_timeout", &self.connect_timeout)
            .field("command_timeout", &self.command_timeout)
            .finish()
    }
}

// Ported from nodemailer's test/shared/url-test.js "parity with shared.parseConnectionUrl".
#[cfg(test)]
mod tests {
    use super::*;

    fn auth_of(config: &SmtpConfig) -> (&str, &str) {
        match &config.auth {
            Some(Auth::Plain { username, password }) => (username.as_str(), password.as_str()),
            _ => panic!("expected Auth::Plain"),
        }
    }

    #[test]
    fn parses_smtps_url_with_auth() {
        let config = SmtpConfig::from_url("smtps://user:pass@localhost:123").unwrap();
        assert!(config.secure);
        assert_eq!(config.port, 123);
        assert_eq!(config.host, "localhost");
        assert_eq!(auth_of(&config), ("user", "pass"));
    }

    #[test]
    fn parses_smtp_url_without_scheme_defaulting_to_insecure() {
        let config = SmtpConfig::from_url("smtp://testuser:testpass@www.example.com:1234").unwrap();
        assert!(!config.secure);
        assert_eq!(config.port, 1234);
        assert_eq!(config.host, "www.example.com");
        assert_eq!(auth_of(&config), ("testuser", "testpass"));
    }

    #[test]
    fn decodes_percent_encoded_auth() {
        let config =
            SmtpConfig::from_url("smtps://user%40gmail.com:%3Apasswith%25Char@smtp.gmail.com")
                .unwrap();
        assert_eq!(config.host, "smtp.gmail.com");
        assert_eq!(auth_of(&config), ("user@gmail.com", ":passwith%Char"));
    }

    #[test]
    fn keeps_a_password_only_credential() {
        let config = SmtpConfig::from_url("smtps://:my-api-key@smtp.host:465").unwrap();
        assert_eq!(config.port, 465);
        assert_eq!(config.host, "smtp.host");
        assert_eq!(auth_of(&config), ("", "my-api-key"));
    }

    #[test]
    fn defaults_port_by_scheme_when_omitted() {
        assert_eq!(
            SmtpConfig::from_url("smtps://smtp.example.com")
                .unwrap()
                .port,
            465
        );
        assert_eq!(
            SmtpConfig::from_url("smtp://smtp.example.com")
                .unwrap()
                .port,
            587
        );
    }

    #[test]
    fn rejects_unsupported_scheme() {
        assert!(SmtpConfig::from_url("http://example.com").is_err());
    }

    #[test]
    fn rejects_url_without_scheme_separator() {
        assert!(SmtpConfig::from_url("not a url").is_err());
    }

    #[test]
    fn percent_decode_handles_percent_before_multibyte_utf8_without_panicking() {
        // "%" followed by a multi-byte UTF-8 char ('€' = 0xE2 0x82 0xAC) used to
        // panic: `s[i+1..i+3]` sliced across the character's byte boundary instead of
        // bailing out once the following bytes weren't ASCII hex digits.
        assert_eq!(percent_decode("a%\u{20AC}b"), "a%\u{20AC}b");
    }

    #[test]
    fn from_url_does_not_panic_on_percent_before_multibyte_utf8_in_credentials() {
        let result = SmtpConfig::from_url("smtp://user:%\u{20AC}pass@smtp.host");
        assert!(result.is_ok());
    }

    // Ported from test/smtp-connection/smtp-connection-test.js: EHLO name is written
    // directly into an SMTP command, so CRLF must be stripped (GHSA-class SMTP
    // command injection), not merely escaped.
    #[test]
    fn ehlo_name_strips_crlf_injection_attempt() {
        let raw = "legit.host\r\nMAIL FROM:<attacker@evil.com>\r\nRCPT TO:<victim@target.com>";
        let config = SmtpConfig::new("localhost", 587).with_ehlo_name(raw);
        assert_eq!(
            config.ehlo_name,
            "legit.hostMAIL FROM:<attacker@evil.com>RCPT TO:<victim@target.com>"
        );
        assert!(!config.ehlo_name.contains('\r'));
        assert!(!config.ehlo_name.contains('\n'));
    }

    #[test]
    fn ehlo_name_strips_bare_cr_bare_lf_and_runs() {
        assert_eq!(
            SmtpConfig::new("h", 587).with_ehlo_name("a\rb").ehlo_name,
            "ab"
        );
        assert_eq!(
            SmtpConfig::new("h", 587).with_ehlo_name("a\nb").ehlo_name,
            "ab"
        );
        assert_eq!(
            SmtpConfig::new("h", 587)
                .with_ehlo_name("a\r\n\r\nb")
                .ehlo_name,
            "ab"
        );
    }

    #[test]
    fn ehlo_name_preserves_clean_value() {
        assert_eq!(
            SmtpConfig::new("h", 587)
                .with_ehlo_name("mail.example.com")
                .ehlo_name,
            "mail.example.com"
        );
    }
}
