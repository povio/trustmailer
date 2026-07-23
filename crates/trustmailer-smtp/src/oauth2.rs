use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use trustmailer_core::{Error, Result};
use trustmailer_smtp_protocol::Auth;

/// Refresh the cached access token this long before it actually expires, to absorb
/// clock skew and in-flight request latency.
const EXPIRY_SAFETY_MARGIN: Duration = Duration::from_secs(60);

/// Config for refreshing an OAuth2 access token (XOAUTH2), e.g. for Gmail: register an
/// OAuth app, obtain a `refresh_token` once via the consent flow, then let this exchange
/// it for short-lived access tokens as needed — mirrors nodemailer's `lib/xoauth2`.
#[derive(Clone)]
pub struct OAuth2Config {
    pub user: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    /// Token endpoint; defaults to Google's.
    pub access_url: String,
}

/// Hand-written rather than derived: `client_secret`/`refresh_token` are plaintext
/// secrets and must not leak through `{:?}` logging.
impl std::fmt::Debug for OAuth2Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth2Config")
            .field("user", &self.user)
            .field("client_id", &self.client_id)
            .field("client_secret", &"***REDACTED***")
            .field("refresh_token", &"***REDACTED***")
            .field("access_url", &self.access_url)
            .finish()
    }
}

impl OAuth2Config {
    pub fn new(
        user: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        refresh_token: impl Into<String>,
    ) -> Self {
        Self {
            user: user.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            refresh_token: refresh_token.into(),
            access_url: "https://accounts.google.com/o/oauth2/token".to_string(),
        }
    }

    pub fn with_access_url(mut self, url: impl Into<String>) -> Self {
        self.access_url = url.into();
        self
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Fetches fresh XOAUTH2 access tokens on demand from a refresh token, caching the
/// result until it's near/past expiry so pooled connections don't each re-hit the
/// OAuth endpoint for their own token.
pub struct OAuth2TokenProvider {
    config: OAuth2Config,
    http: reqwest::Client,
    cached: Mutex<Option<CachedToken>>,
}

/// Rejects a non-HTTPS `access_url` up front, except for loopback addresses (used by
/// this crate's own tests against a local mock token endpoint) — sending
/// `client_secret`/`refresh_token` to a misconfigured plaintext HTTP URL would leak
/// them on the wire.
fn is_https_or_loopback(url: &str) -> bool {
    if url.starts_with("https://") {
        return true;
    }
    // The loopback exception must match the *authority* exactly — the host has to
    // terminate at a port (`:`), a path (`/`), or end-of-string. A bare `starts_with`
    // would let `http://127.0.0.1.evil.com` or `http://localhost@evil.com` through and
    // POST `client_secret`/`refresh_token` in cleartext to an attacker-controlled host.
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    for host in ["127.0.0.1", "localhost", "[::1]"] {
        if let Some(after) = rest.strip_prefix(host) {
            if after.is_empty() || after.starts_with([':', '/']) {
                return true;
            }
        }
    }
    false
}

impl OAuth2TokenProvider {
    pub fn new(config: OAuth2Config) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            cached: Mutex::new(None),
        }
    }

    /// Exchanges the refresh token for a new access token. Always hits the network —
    /// use [`OAuth2TokenProvider::get_access_token`] (or [`Self::to_auth`]) to benefit
    /// from caching.
    pub async fn fetch_access_token(&self) -> Result<String> {
        Ok(self.fetch_token_response().await?.access_token)
    }

    async fn fetch_token_response(&self) -> Result<TokenResponse> {
        if !is_https_or_loopback(&self.config.access_url) {
            return Err(Error::Transport(format!(
                "refusing to send OAuth2 credentials to a non-HTTPS access_url: {}",
                self.config.access_url
            )));
        }

        let params = [
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
            ("refresh_token", self.config.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .http
            .post(&self.config.access_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| Error::Transport(format!("OAuth2 token request failed: {e}")))?;

        let status = response.status();
        let body: TokenResponse = response.json().await.map_err(|e| {
            Error::Transport(format!("OAuth2 token response was not valid JSON: {e}"))
        })?;

        if let Some(err) = &body.error {
            let desc = body.error_description.clone().unwrap_or_default();
            return Err(Error::Transport(format!(
                "OAuth2 token refresh failed: {err} {desc}"
            )));
        }
        if !status.is_success() {
            return Err(Error::Transport(format!(
                "OAuth2 token endpoint returned {status}"
            )));
        }

        Ok(body)
    }

    /// Returns a cached access token if it's still fresh (with a safety margin before
    /// its actual expiry), otherwise fetches and caches a new one.
    pub async fn get_access_token(&self) -> Result<String> {
        {
            let cache = self.cached.lock().await;
            if let Some(cached) = cache.as_ref() {
                if cached.expires_at > Instant::now() {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        let body = self.fetch_token_response().await?;
        let ttl = Duration::from_secs(body.expires_in.unwrap_or(0));
        let expires_at = Instant::now() + ttl.saturating_sub(EXPIRY_SAFETY_MARGIN);

        let mut cache = self.cached.lock().await;
        *cache = Some(CachedToken {
            access_token: body.access_token.clone(),
            expires_at,
        });
        Ok(body.access_token)
    }

    /// Fetches a (possibly cached) access token and wraps it as an [`Auth::XOAuth2`]
    /// ready to hand to [`crate::SmtpConfig::with_auth`].
    pub async fn to_auth(&self) -> Result<Auth> {
        let access_token = self.get_access_token().await?;
        Ok(Auth::XOAuth2 {
            username: self.config.user.clone(),
            access_token,
        })
    }
}

// Behavioral ports of test/xoauth2/xoauth2-test.js's token-refresh cases, against a
// hand-rolled mock token endpoint (same technique as trustmailer-ses's tests).
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn mock_token_endpoint(status: u16, body: &'static str) -> String {
        let (url, _count) = mock_token_endpoint_counting(status, body).await;
        url
    }

    /// Like `mock_token_endpoint`, but serves every connection it receives (rather
    /// than exactly one) and returns a counter of how many requests it handled — used
    /// to assert caching behavior (a cached token must not trigger a second request).
    async fn mock_token_endpoint_counting(status: u16, body: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => return,
                };
                count_clone.fetch_add(1, Ordering::SeqCst);
                let reason = if status == 200 { "OK" } else { "Bad Request" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let _ = socket.read(&mut buf).await;
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (format!("http://127.0.0.1:{port}/token"), count)
    }

    #[test]
    fn loopback_exception_requires_an_exact_authority() {
        // Legitimate.
        assert!(is_https_or_loopback("https://oauth2.googleapis.com/token"));
        assert!(is_https_or_loopback("http://127.0.0.1:8080/token"));
        assert!(is_https_or_loopback("http://localhost/token"));
        assert!(is_https_or_loopback("http://[::1]:9000/token"));
        assert!(is_https_or_loopback("http://127.0.0.1"));
        // Bypass attempts that a bare `starts_with` would have let through.
        assert!(!is_https_or_loopback("http://127.0.0.1.evil.com/token"));
        assert!(!is_https_or_loopback("http://localhost.evil.com/token"));
        assert!(!is_https_or_loopback("http://127.0.0.1@evil.com/token"));
        assert!(!is_https_or_loopback("http://localhostx/token"));
        assert!(!is_https_or_loopback("http://evil.com/token"));
    }

    #[tokio::test]
    async fn refreshes_an_access_token() {
        let url = mock_token_endpoint(
            200,
            r#"{"access_token":"ya29.test-access-token","expires_in":3600}"#,
        )
        .await;
        let provider = OAuth2TokenProvider::new(
            OAuth2Config::new(
                "user@gmail.com",
                "client-id",
                "client-secret",
                "refresh-token",
            )
            .with_access_url(url),
        );

        let token = provider.fetch_access_token().await.unwrap();
        assert_eq!(token, "ya29.test-access-token");
    }

    #[tokio::test]
    async fn wraps_the_token_as_xoauth2_auth() {
        let url = mock_token_endpoint(200, r#"{"access_token":"ya29.test-access-token"}"#).await;
        let provider = OAuth2TokenProvider::new(
            OAuth2Config::new(
                "user@gmail.com",
                "client-id",
                "client-secret",
                "refresh-token",
            )
            .with_access_url(url),
        );

        let auth = provider.to_auth().await.unwrap();
        match auth {
            Auth::XOAuth2 {
                username,
                access_token,
            } => {
                assert_eq!(username, "user@gmail.com");
                assert_eq!(access_token, "ya29.test-access-token");
            }
            _ => panic!("expected XOAuth2"),
        }
    }

    #[tokio::test]
    async fn surfaces_an_oauth_error_response() {
        let url = mock_token_endpoint(
            400,
            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked"}"#,
        )
        .await;
        let provider = OAuth2TokenProvider::new(
            OAuth2Config::new(
                "user@gmail.com",
                "client-id",
                "client-secret",
                "refresh-token",
            )
            .with_access_url(url),
        );

        let err = provider.fetch_access_token().await.unwrap_err();
        assert!(err.to_string().contains("invalid_grant"));
    }

    #[tokio::test]
    async fn caches_the_access_token_until_near_expiry() {
        let (url, count) = mock_token_endpoint_counting(
            200,
            r#"{"access_token":"ya29.cached-token","expires_in":3600}"#,
        )
        .await;
        let provider = OAuth2TokenProvider::new(
            OAuth2Config::new("user@gmail.com", "client-id", "client-secret", "refresh-token")
                .with_access_url(url),
        );

        let first = provider.get_access_token().await.unwrap();
        let second = provider.get_access_token().await.unwrap();

        assert_eq!(first, "ya29.cached-token");
        assert_eq!(second, "ya29.cached-token");
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "second call within the token's lifetime should be served from cache, not refetched"
        );
    }

    #[tokio::test]
    async fn refetches_once_the_cached_token_is_near_expiry() {
        // expires_in of 1s combined with the 60s safety margin means the cached token
        // is already considered "near expiry" immediately — every call should refetch.
        let (url, count) =
            mock_token_endpoint_counting(200, r#"{"access_token":"ya29.short-lived","expires_in":1}"#)
                .await;
        let provider = OAuth2TokenProvider::new(
            OAuth2Config::new("user@gmail.com", "client-id", "client-secret", "refresh-token")
                .with_access_url(url),
        );

        provider.get_access_token().await.unwrap();
        provider.get_access_token().await.unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn rejects_a_non_https_non_loopback_access_url() {
        let provider = OAuth2TokenProvider::new(
            OAuth2Config::new("user@gmail.com", "client-id", "client-secret", "refresh-token")
                .with_access_url("http://evil.example.com/token"),
        );

        let err = provider.fetch_access_token().await.unwrap_err();
        assert!(err.to_string().to_lowercase().contains("https"));
    }
}
