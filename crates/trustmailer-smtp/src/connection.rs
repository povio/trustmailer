use crate::config::SmtpConfig;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_socks::tcp::Socks5Stream;
use tokio_util::either::Either;
use trustmailer_core::{Error, Result};
use trustmailer_smtp_protocol::{
    auth_command, dot_stuff, ehlo_command, mail_from_command, parse_ehlo_capabilities,
    rcpt_to_command, validate_envelope_address, Auth, ResponseParser,
};

/// Direct TCP, or tunneled through a SOCKS5 proxy.
type RawStream = Either<TcpStream, Socks5Stream<TcpStream>>;
type Stream = Either<RawStream, tokio_rustls::client::TlsStream<RawStream>>;

/// Hard cap on a single response *line*'s length (before a `\n` is found), so a server
/// that never terminates a line can't grow the read buffer unbounded.
const MAX_RESPONSE_LINE_BYTES: usize = 64 * 1024;

/// Runs `fut`, turning a timeout into a clear `Error::Transport` instead of hanging
/// forever — every connect/handshake/write/read in this module goes through this.
async fn with_timeout<T>(duration: Duration, fut: impl Future<Output = Result<T>>) -> Result<T> {
    match tokio::time::timeout(duration, fut).await {
        Ok(result) => result,
        Err(_) => Err(Error::Transport(format!(
            "operation timed out after {duration:?}"
        ))),
    }
}

async fn connect_raw(config: &SmtpConfig) -> Result<RawStream> {
    match &config.proxy {
        None => {
            let tcp = TcpStream::connect((config.host.as_str(), config.port))
                .await
                .map_err(|e| {
                    Error::Transport(format!(
                        "connect to {}:{} failed: {e}",
                        config.host, config.port
                    ))
                })?;
            Ok(Either::Left(tcp))
        }
        Some(proxy) => {
            let target = (config.host.as_str(), config.port);
            let proxy_addr = (proxy.host.as_str(), proxy.port);
            let stream = match (&proxy.username, &proxy.password) {
                (Some(user), Some(pass)) => {
                    Socks5Stream::connect_with_password(proxy_addr, target, user, pass)
                        .await
                        .map_err(|e| {
                            Error::Transport(format!(
                                "SOCKS5 proxy {}:{} failed: {e}",
                                proxy.host, proxy.port
                            ))
                        })?
                }
                (None, None) => Socks5Stream::connect(proxy_addr, target).await.map_err(|e| {
                    Error::Transport(format!(
                        "SOCKS5 proxy {}:{} failed: {e}",
                        proxy.host, proxy.port
                    ))
                })?,
                _ => {
                    // Exactly one of username/password is set. Silently falling back to
                    // anonymous auth here would be a security downgrade the caller
                    // never asked for (`ProxyConfig`'s fields are `pub`, so this is
                    // directly constructible, bypassing `with_auth`) — treat it as a
                    // hard config error instead.
                    return Err(Error::Transport(
                        "SOCKS5 proxy config has only one of username/password set — set both (via `ProxyConfig::with_auth`) or neither".into(),
                    ));
                }
            };
            Ok(Either::Right(stream))
        }
    }
}

/// A single, already-authenticated SMTP connection. One `send_message` per instance
/// unless `mail_reset` is used to start a fresh transaction on the same connection.
pub struct SmtpConnection {
    stream: BufReader<Stream>,
    command_timeout: Duration,
}

fn tls_connector() -> TlsConnector {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

impl SmtpConnection {
    pub async fn connect(config: &SmtpConfig) -> Result<Self> {
        let tcp = with_timeout(config.connect_timeout, connect_raw(config)).await?;

        let mut stream = if config.secure {
            let tls = with_timeout(config.connect_timeout, async {
                tls_connector()
                    .connect(server_name(&config.host)?, tcp)
                    .await
                    .map_err(|e| Error::Transport(format!("TLS handshake failed: {e}")))
            })
            .await?;
            BufReader::new(Either::Right(tls))
        } else {
            BufReader::new(Either::Left(tcp))
        };

        read_greeting(&mut stream, config.command_timeout).await?;
        let mut caps = ehlo(&mut stream, &config.ehlo_name, config.command_timeout).await?;

        // Whether the channel is actually protected by TLS at this point — either
        // implicit TLS from the start, or STARTTLS completed below.
        let mut secure_channel = config.secure;

        if !config.secure {
            let advertises_starttls = caps.iter().any(|c| c.eq_ignore_ascii_case("STARTTLS"));
            if config.require_tls && !advertises_starttls {
                return Err(Error::Transport(
                    "server does not advertise STARTTLS".into(),
                ));
            }
            // Opportunistic STARTTLS: upgrade whenever the server offers it, even if
            // `require_tls` wasn't explicitly requested — don't leave a connection
            // plaintext just because the caller didn't ask, when TLS is available.
            // A caller can decline the upgrade with `disable_starttls` (e.g. an internal
            // relay with an untrusted cert), but `require_tls` still forces it.
            if advertises_starttls && (config.require_tls || !config.disable_starttls) {
                send_line(&mut stream, "STARTTLS", config.command_timeout).await?;
                expect_code(&mut stream, 220, config.command_timeout).await?;

                let Either::Left(tcp) = stream.into_inner() else {
                    unreachable!("STARTTLS is only performed on a plaintext connection")
                };
                let tls = with_timeout(config.connect_timeout, async {
                    tls_connector()
                        .connect(server_name(&config.host)?, tcp)
                        .await
                        .map_err(|e| Error::Transport(format!("STARTTLS handshake failed: {e}")))
                })
                .await?;
                stream = BufReader::new(Either::Right(tls));
                caps = ehlo(&mut stream, &config.ehlo_name, config.command_timeout).await?;
                secure_channel = true;
            }
        }

        let mut conn = Self {
            stream,
            command_timeout: config.command_timeout,
        };

        if let Some(auth) = &config.auth {
            // Refuse to send credentials over a channel that isn't secure unless the
            // caller has explicitly opted in — this is the one guard that actually
            // stops `authenticate()` from running over plaintext on a misconfigured
            // (e.g. non-standard-port) relay.
            if !secure_channel && !config.allow_insecure_auth {
                return Err(Error::Transport(
                    "refusing to send AUTH over a non-TLS connection (set SmtpConfig::allow_insecure_auth to override)".into(),
                ));
            }

            let mechanism = auth.mechanism_name();
            let advertised = caps.iter().any(|c| {
                let upper = c.to_ascii_uppercase();
                upper.starts_with("AUTH") && upper.split_whitespace().skip(1).any(|m| m == mechanism)
            });
            if !advertised {
                return Err(Error::Transport(format!(
                    "server does not advertise AUTH {mechanism}"
                )));
            }
            conn.authenticate(auth).await?;
        }

        Ok(conn)
    }

    async fn authenticate(&mut self, auth: &Auth) -> Result<()> {
        let timeout = self.command_timeout;
        match auth {
            Auth::Plain { .. } | Auth::XOAuth2 { .. } => {
                send_line(&mut self.stream, &auth_command(auth), timeout).await?;
                expect_code(&mut self.stream, 235, timeout).await?;
            }
            Auth::Login { username, password } => {
                send_line(&mut self.stream, "AUTH LOGIN", timeout).await?;
                expect_code(&mut self.stream, 334, timeout).await?;
                send_line(&mut self.stream, &Auth::encode(username), timeout).await?;
                expect_code(&mut self.stream, 334, timeout).await?;
                send_line(&mut self.stream, &Auth::encode(password), timeout).await?;
                expect_code(&mut self.stream, 235, timeout).await?;
            }
        }
        Ok(())
    }

    pub async fn send_transaction(
        &mut self,
        from: &str,
        to: &[String],
        data: &[u8],
    ) -> Result<String> {
        let timeout = self.command_timeout;

        // Security: `from`/`to` ultimately come from parsed address strings, which our
        // permissive address parser doesn't reject for containing control characters.
        // Refuse anything that could break out of `MAIL FROM:<...>`/`RCPT TO:<...>` and
        // inject a second SMTP command, rather than trying to escape it.
        validate_envelope_address(from, "sender")?;
        send_line(&mut self.stream, &mail_from_command(from), timeout).await?;
        expect_code(&mut self.stream, 250, timeout).await?;

        for rcpt in to {
            validate_envelope_address(rcpt, "recipient")?;
            send_line(&mut self.stream, &rcpt_to_command(rcpt), timeout).await?;
            expect_code(&mut self.stream, 250, timeout).await?;
        }

        send_line(&mut self.stream, "DATA", timeout).await?;
        expect_code(&mut self.stream, 354, timeout).await?;

        let stream = &mut self.stream;
        with_timeout(timeout, async {
            stream.get_mut().write_all(&dot_stuff(data)).await.map_err(io_err)?;
            stream.get_mut().write_all(b"\r\n.\r\n").await.map_err(io_err)?;
            stream.get_mut().flush().await.map_err(io_err)?;
            Ok(())
        })
        .await?;

        let (code, text) = read_response(&mut self.stream, timeout).await?;
        if code != 250 {
            return Err(Error::Transport(format!(
                "server rejected message: {code} {text}"
            )));
        }
        Ok(text)
    }

    pub async fn quit(mut self) -> Result<()> {
        let timeout = self.command_timeout;
        let _ = send_line(&mut self.stream, "QUIT", timeout).await;
        Ok(())
    }

    /// Sends a `NOOP` to confirm the connection/authentication is still healthy.
    pub async fn noop(&mut self) -> Result<()> {
        let timeout = self.command_timeout;
        send_line(&mut self.stream, "NOOP", timeout).await?;
        expect_code(&mut self.stream, 250, timeout).await?;
        Ok(())
    }
}

fn server_name(host: &str) -> Result<rustls_pki_types::ServerName<'static>> {
    rustls_pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| Error::Transport(format!("invalid TLS server name {host}: {e}")))
}

fn io_err(e: std::io::Error) -> Error {
    Error::Transport(format!("connection error: {e}"))
}

async fn read_greeting(stream: &mut BufReader<Stream>, timeout: Duration) -> Result<()> {
    let (code, _) = read_response(stream, timeout).await?;
    if code != 220 {
        return Err(Error::Transport(format!(
            "unexpected greeting code: {code}"
        )));
    }
    Ok(())
}

async fn ehlo(
    stream: &mut BufReader<Stream>,
    ehlo_name: &str,
    timeout: Duration,
) -> Result<Vec<String>> {
    send_line(stream, &ehlo_command(ehlo_name), timeout).await?;
    let (code, text) = read_response(stream, timeout).await?;
    if code != 250 {
        return Err(Error::Transport(format!("EHLO rejected: {code} {text}")));
    }
    Ok(parse_ehlo_capabilities(&text))
}

/// Writes `line` + CRLF as a single buffered `write_all` (rather than two separate
/// writes) followed by one flush.
async fn send_line(stream: &mut BufReader<Stream>, line: &str, timeout: Duration) -> Result<()> {
    with_timeout(timeout, async {
        let mut buf = Vec::with_capacity(line.len() + 2);
        buf.extend_from_slice(line.as_bytes());
        buf.extend_from_slice(b"\r\n");
        stream.get_mut().write_all(&buf).await.map_err(io_err)?;
        stream.get_mut().flush().await.map_err(io_err)?;
        Ok(())
    })
    .await
}

async fn expect_code(
    stream: &mut BufReader<Stream>,
    expected: u16,
    timeout: Duration,
) -> Result<String> {
    let (code, text) = read_response(stream, timeout).await?;
    if code != expected {
        return Err(Error::Transport(format!(
            "expected SMTP code {expected}, got {code}: {text}"
        )));
    }
    Ok(text)
}

/// Reads a (possibly multi-line, `250-...` continued) SMTP response and returns
/// `(status_code, joined_text_without_codes)`. Bounded on three axes: per-line length
/// (`MAX_RESPONSE_LINE_BYTES`, via a `.take()`-limited reader), total line count and
/// total byte size (both enforced by `ResponseParser`), and total wall-clock time
/// (`timeout`) — a malicious/misbehaving server can't block forever or grow client
/// memory unbounded.
async fn read_response(stream: &mut BufReader<Stream>, timeout: Duration) -> Result<(u16, String)> {
    with_timeout(timeout, async {
        let mut parser = ResponseParser::new();
        loop {
            let mut line = String::new();
            let n = {
                let mut limited = (&mut *stream).take(MAX_RESPONSE_LINE_BYTES as u64);
                limited.read_line(&mut line).await.map_err(io_err)?
            };
            if n == 0 {
                return Err(Error::Transport("connection closed unexpectedly".into()));
            }
            if !line.ends_with('\n') {
                // Either the `.take()` budget ran out before a newline showed up (line
                // too long) or the connection was closed mid-line; either way, don't
                // keep growing the buffer.
                return Err(Error::Transport(
                    "SMTP response line exceeded maximum length".into(),
                ));
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(result) = parser.feed_line(line)? {
                return Ok(result);
            }
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::ProxyConfig;

    // Ported intent from the audit's SOCKS5 fail-open finding: `ProxyConfig`'s fields
    // are `pub`, so a caller can construct a config with only one of username/password
    // set (bypassing `with_auth`) — this must be a hard config error, not a silent
    // downgrade to anonymous SOCKS5 auth.
    #[tokio::test]
    async fn socks5_proxy_with_only_username_set_is_a_config_error() {
        let mut config = SmtpConfig::new("example.com", 25);
        let proxy = ProxyConfig {
            host: "127.0.0.1".to_string(),
            port: 1080,
            username: Some("user".to_string()),
            password: None,
        };
        config.proxy = Some(proxy);

        let err = connect_raw(&config).await.unwrap_err();
        assert!(err.to_string().to_lowercase().contains("proxy"));
    }

    #[tokio::test]
    async fn socks5_proxy_with_only_password_set_is_a_config_error() {
        let mut config = SmtpConfig::new("example.com", 25);
        let proxy = ProxyConfig {
            host: "127.0.0.1".to_string(),
            port: 1080,
            username: None,
            password: Some("pass".to_string()),
        };
        config.proxy = Some(proxy);

        let err = connect_raw(&config).await.unwrap_err();
        assert!(err.to_string().to_lowercase().contains("proxy"));
    }
}
