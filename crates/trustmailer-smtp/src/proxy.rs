/// SOCKS5 proxy to tunnel the SMTP connection through, mirroring nodemailer's SMTP
/// `proxy: "socks5://host:port"` option (HTTP CONNECT proxies aren't supported yet).
#[derive(Clone)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Hand-written rather than derived: `password` is a plaintext secret and must not
/// leak through `{:?}` logging.
impl std::fmt::Debug for ProxyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field(
                "password",
                &self.password.as_ref().map(|_| "***REDACTED***"),
            )
            .finish()
    }
}

impl ProxyConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            username: None,
            password: None,
        }
    }

    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    /// Parses a `socks5://[user:pass@]host:port` URL.
    pub fn parse(url: &str) -> Option<Self> {
        let rest = url
            .strip_prefix("socks5://")
            .or_else(|| url.strip_prefix("socks5h://"))?;
        let (auth, hostport) = match rest.rsplit_once('@') {
            Some((auth, hostport)) => (Some(auth), hostport),
            None => (None, rest),
        };
        let (host, port) = hostport.rsplit_once(':')?;
        let port: u16 = port.parse().ok()?;

        let mut cfg = ProxyConfig::new(host, port);
        if let Some(auth) = auth {
            let (user, pass) = auth.split_once(':').unwrap_or((auth, ""));
            cfg = cfg.with_auth(user, pass);
        }
        Some(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_socks5_url() {
        let cfg = ProxyConfig::parse("socks5://proxy.example.com:1080").unwrap();
        assert_eq!(cfg.host, "proxy.example.com");
        assert_eq!(cfg.port, 1080);
        assert!(cfg.username.is_none());
    }

    #[test]
    fn parses_socks5_url_with_credentials() {
        let cfg = ProxyConfig::parse("socks5://user:pass@proxy.example.com:1080").unwrap();
        assert_eq!(cfg.username.as_deref(), Some("user"));
        assert_eq!(cfg.password.as_deref(), Some("pass"));
    }

    #[test]
    fn rejects_non_socks5_scheme() {
        assert!(ProxyConfig::parse("http://proxy.example.com:8080").is_none());
    }
}
