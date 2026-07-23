//! Integration tests against a small hand-rolled plaintext SMTP server, standing in
//! for the real `smtp-server` npm package nodemailer's own test suite spins up.
//! Behavioral ports of test/smtp-transport/smtp-tranport-test.js,
//! test/smtp-connection/smtp-connection-test.js and test/smtp-pool/smtp-pool-test.js.
//!
//! TLS/STARTTLS isn't exercised here (the fake server is plaintext-only) — configs
//! use `secure: false` with a non-587 port so `SmtpConnection` never attempts it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use trustmailer_core::{Message, Transport};
use trustmailer_smtp::{Auth, SmtpConfig, SmtpTransport};

#[derive(Clone, Default)]
struct ServerBehavior {
    require_auth: bool,
    expected_user: Option<String>,
    expected_pass: Option<String>,
    reject_rcpt: Option<String>,
    reject_data: bool,
    /// Advertise only `AUTH LOGIN` (not PLAIN) — for the mechanism-specific
    /// pre-check test.
    login_only: bool,
    /// Close the connection immediately after a successful DATA response, simulating
    /// a server-side idle timeout — for the stale-pooled-connection test.
    close_after_send: bool,
    /// Accept the TCP connection but never write anything (not even the greeting) —
    /// for the command-timeout test.
    hang_after_accept: bool,
}

/// Starts a fake SMTP server on an ephemeral localhost port and returns
/// `(port, connection_count)`. Accepts connections until the test process exits.
async fn start_fake_server(behavior: ServerBehavior) -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connection_count = Arc::new(AtomicUsize::new(0));
    let count_clone = connection_count.clone();

    tokio::spawn(async move {
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => return,
            };
            count_clone.fetch_add(1, Ordering::SeqCst);
            let behavior = behavior.clone();
            tokio::spawn(handle_connection(socket, behavior));
        }
    });

    (port, connection_count)
}

async fn handle_connection(socket: TcpStream, behavior: ServerBehavior) {
    if behavior.hang_after_accept {
        // Simulate a wedged/dead server: accept the TCP connection but never send the
        // greeting, so the client-side command_timeout is what has to save us.
        std::future::pending::<()>().await;
        return;
    }

    let (read_half, mut write_half) = socket.into_split();
    let mut reader = BufReader::new(read_half);

    let _ = write_half.write_all(b"220 fake.smtp ESMTP\r\n").await;

    let mut authenticated = !behavior.require_auth;
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        let cmd = line.trim_end();
        let upper = cmd.to_ascii_uppercase();

        if upper.starts_with("EHLO") {
            let mut caps = "250-fake.smtp\r\n".to_string();
            if behavior.require_auth {
                if behavior.login_only {
                    caps.push_str("250-AUTH LOGIN\r\n");
                } else {
                    caps.push_str("250-AUTH PLAIN LOGIN\r\n");
                }
            }
            caps.push_str("250 PIPELINING\r\n");
            let _ = write_half.write_all(caps.as_bytes()).await;
        } else if upper.starts_with("AUTH PLAIN") {
            let arg = cmd.splitn(3, ' ').nth(2).unwrap_or("");
            let ok = check_auth_plain(arg, &behavior);
            let resp = if ok {
                "235 Authentication successful\r\n"
            } else {
                "535 Authentication failed\r\n"
            };
            authenticated = ok;
            let _ = write_half.write_all(resp.as_bytes()).await;
        } else if upper.starts_with("MAIL FROM") {
            if behavior.require_auth && !authenticated {
                let _ = write_half
                    .write_all(b"530 Authentication required\r\n")
                    .await;
            } else {
                let _ = write_half.write_all(b"250 OK\r\n").await;
            }
        } else if upper.starts_with("RCPT TO") {
            let rejected = behavior
                .reject_rcpt
                .as_deref()
                .map(|r| cmd.contains(r))
                .unwrap_or(false);
            if rejected {
                let _ = write_half.write_all(b"550 No such user\r\n").await;
            } else {
                let _ = write_half.write_all(b"250 OK\r\n").await;
            }
        } else if upper.starts_with("DATA") {
            let _ = write_half.write_all(b"354 Start mail input\r\n").await;
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await.unwrap_or(0);
                if n == 0 || line == ".\r\n" {
                    break;
                }
            }
            if behavior.reject_data {
                let _ = write_half.write_all(b"554 Transaction failed\r\n").await;
            } else {
                let _ = write_half.write_all(b"250 OK: queued as 12345\r\n").await;
                if behavior.close_after_send {
                    // Simulate the server dropping the connection right after
                    // (idle-timeout-style) instead of waiting for NOOP/QUIT.
                    return;
                }
            }
        } else if upper.starts_with("NOOP") {
            let _ = write_half.write_all(b"250 OK\r\n").await;
        } else if upper.starts_with("QUIT") {
            let _ = write_half.write_all(b"221 Bye\r\n").await;
            return;
        } else {
            let _ = write_half.write_all(b"500 unrecognized command\r\n").await;
        }
    }
}

fn check_auth_plain(b64_arg: &str, behavior: &ServerBehavior) -> bool {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let Ok(decoded) = STANDARD.decode(b64_arg) else {
        return false;
    };
    let Ok(text) = String::from_utf8(decoded) else {
        return false;
    };
    let mut parts = text.split('\0');
    let _authzid = parts.next();
    let user = parts.next().unwrap_or("");
    let pass = parts.next().unwrap_or("");
    behavior.expected_user.as_deref() == Some(user)
        && behavior.expected_pass.as_deref() == Some(pass)
}

fn test_message() -> Message {
    Message::builder()
        .from("sender@example.com")
        .to("recipient@example.com")
        .subject("hi")
        .text("hello world")
        .build()
}

// Port of "Should send mail".
#[tokio::test]
async fn sends_mail_through_the_full_smtp_transaction() {
    let (port, _) = start_fake_server(ServerBehavior::default()).await;
    let config = SmtpConfig::new("127.0.0.1", port);
    let transport = SmtpTransport::new(config);

    let info = transport.send(&test_message()).await.unwrap();
    assert_eq!(info.accepted, vec!["recipient@example.com".to_string()]);
    assert!(info.response.unwrap().contains("queued"));
}

// Port of "Should verify connection ... with success".
#[tokio::test]
async fn verify_succeeds_against_a_healthy_server() {
    let (port, _) = start_fake_server(ServerBehavior::default()).await;
    let config = SmtpConfig::new("127.0.0.1", port);
    let transport = SmtpTransport::new(config);
    assert!(transport.verify().await.unwrap());
}

// Port of "Should not fail auth" / "Should login and send mail".
#[tokio::test]
async fn authenticates_with_plain_before_sending() {
    let behavior = ServerBehavior {
        require_auth: true,
        expected_user: Some("testuser".to_string()),
        expected_pass: Some("testpass".to_string()),
        ..Default::default()
    };
    let (port, _) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port)
        .with_allow_insecure_auth(true)
        .with_auth(Auth::Plain {
            username: "testuser".to_string(),
            password: "testpass".to_string(),
        });
    let transport = SmtpTransport::new(config);

    let info = transport.send(&test_message()).await.unwrap();
    assert_eq!(info.accepted, vec!["recipient@example.com".to_string()]);
}

// Port of "Should fail auth if forceAuth=true" (wrong credentials rejected).
#[tokio::test]
async fn wrong_credentials_are_rejected() {
    let behavior = ServerBehavior {
        require_auth: true,
        expected_user: Some("testuser".to_string()),
        expected_pass: Some("testpass".to_string()),
        ..Default::default()
    };
    let (port, _) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port)
        .with_allow_insecure_auth(true)
        .with_auth(Auth::Plain {
            username: "testuser".to_string(),
            password: "wrong".to_string(),
        });
    let transport = SmtpTransport::new(config);

    let err = transport.send(&test_message()).await.unwrap_err();
    assert!(err.to_string().contains("535") || err.to_string().to_lowercase().contains("auth"));
}

// New: AUTH must be refused over a plaintext connection unless the caller explicitly
// opts in via `allow_insecure_auth` (audit §2 finding #5).
#[tokio::test]
async fn auth_over_plaintext_is_refused_without_explicit_opt_in() {
    let behavior = ServerBehavior {
        require_auth: true,
        expected_user: Some("testuser".to_string()),
        expected_pass: Some("testpass".to_string()),
        ..Default::default()
    };
    let (port, _) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port).with_auth(Auth::Plain {
        username: "testuser".to_string(),
        password: "testpass".to_string(),
    });
    let transport = SmtpTransport::new(config);

    let err = transport.send(&test_message()).await.unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("insecure") || msg.contains("tls"),
        "unexpected error message: {msg}"
    );
}

// New: the AUTH pre-check must look for the *specific* configured mechanism, not just
// any `AUTH` capability line (audit §2 finding #10).
#[tokio::test]
async fn auth_precheck_rejects_mechanism_the_server_does_not_advertise() {
    let behavior = ServerBehavior {
        require_auth: true,
        login_only: true,
        expected_user: Some("testuser".to_string()),
        expected_pass: Some("testpass".to_string()),
        ..Default::default()
    };
    let (port, _) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port)
        .with_allow_insecure_auth(true)
        .with_auth(Auth::Plain {
            username: "testuser".to_string(),
            password: "testpass".to_string(),
        });
    let transport = SmtpTransport::new(config);

    let err = transport.send(&test_message()).await.unwrap_err();
    assert!(err.to_string().contains("PLAIN"));
}

// New: SMTP command timeouts must actually fire rather than hanging forever (audit §2
// finding #4).
#[tokio::test]
async fn command_timeout_fires_when_server_never_responds() {
    let behavior = ServerBehavior {
        hang_after_accept: true,
        ..Default::default()
    };
    let (port, _) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port).with_command_timeout(Duration::from_millis(200));
    let transport = SmtpTransport::new(config);

    let start = std::time::Instant::now();
    let err = transport.send(&test_message()).await.unwrap_err();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "should have timed out quickly rather than hanging"
    );
    assert!(err.to_string().to_lowercase().contains("timed out"));
}

// New: a pooled connection that the server closed while idle must be transparently
// replaced rather than surfacing a raw I/O error on the caller's next send (audit §2
// finding #6).
#[tokio::test]
async fn pool_detects_a_stale_idle_connection_and_reconnects_transparently() {
    let behavior = ServerBehavior {
        close_after_send: true,
        ..Default::default()
    };
    let (port, connection_count) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port).with_pool_size(2);
    let pool = SmtpTransport::new(config).pooled();

    pool.send(&test_message()).await.unwrap();
    // The connection just checked back in was closed server-side; this second send
    // must not fail even though the pool's only idle connection is now dead.
    pool.send(&test_message()).await.unwrap();

    assert_eq!(
        connection_count.load(Ordering::SeqCst),
        2,
        "expected exactly one reconnect after the stale connection was detected"
    );
}

// Port of "Should fail envelope" (server rejects RCPT TO).
#[tokio::test]
async fn rejected_recipient_surfaces_as_an_error() {
    let behavior = ServerBehavior {
        reject_rcpt: Some("recipient@example.com".to_string()),
        ..Default::default()
    };
    let (port, _) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port);
    let transport = SmtpTransport::new(config);

    let err = transport.send(&test_message()).await.unwrap_err();
    assert!(err.to_string().contains("550"));
}

// Server rejecting the DATA transaction itself (distinct failure point from RCPT).
#[tokio::test]
async fn rejected_data_transaction_surfaces_as_an_error() {
    let behavior = ServerBehavior {
        reject_data: true,
        ..Default::default()
    };
    let (port, _) = start_fake_server(behavior).await;
    let config = SmtpConfig::new("127.0.0.1", port);
    let transport = SmtpTransport::new(config);

    let err = transport.send(&test_message()).await.unwrap_err();
    assert!(err.to_string().contains("554"));
}

// Pool-specific: sending N messages through a pool with capacity 2 reuses connections
// rather than opening one per send (nodemailer: smtp-pool-test.js connection reuse).
#[tokio::test]
async fn pool_reuses_connections_across_sends() {
    let (port, connection_count) = start_fake_server(ServerBehavior::default()).await;
    let config = SmtpConfig::new("127.0.0.1", port).with_pool_size(2);
    let pool = SmtpTransport::new(config).pooled();

    for _ in 0..5 {
        pool.send(&test_message()).await.unwrap();
    }

    // 5 sequential sends through a pool of idle-reusing connections should open far
    // fewer than 5 raw TCP connections.
    assert!(
        connection_count.load(Ordering::SeqCst) < 5,
        "pool should reuse connections instead of opening a new one per send"
    );
}

#[tokio::test]
async fn pool_send_reports_correct_envelope() {
    let (port, _) = start_fake_server(ServerBehavior::default()).await;
    let config = SmtpConfig::new("127.0.0.1", port);
    let pool = SmtpTransport::new(config).pooled();

    let info = pool.send(&test_message()).await.unwrap();
    assert_eq!(info.envelope.from, "sender@example.com");
    assert_eq!(info.envelope.to, vec!["recipient@example.com".to_string()]);
}

// Ported from test/smtp-connection/smtp-connection-test.js "should not allow injected
// SMTP commands to reach the server via name option" — here via a maliciously crafted
// envelope address instead of the EHLO name (see also config.rs's ehlo_name tests).
#[tokio::test]
async fn envelope_address_with_crlf_is_rejected_before_touching_the_wire() {
    let (port, _) = start_fake_server(ServerBehavior::default()).await;
    let config = SmtpConfig::new("127.0.0.1", port);
    let transport = SmtpTransport::new(config);

    let mut msg = test_message();
    msg.envelope = Some(trustmailer_core::Envelope {
        from: "attacker@evil.com\r\nRCPT TO:<victim@target.com>".to_string(),
        to: vec!["recipient@example.com".to_string()],
    });

    let err = transport.send(&msg).await.unwrap_err();
    assert!(err.to_string().contains("invalid sender"));
}

#[tokio::test]
async fn envelope_recipient_with_crlf_is_rejected_before_touching_the_wire() {
    let (port, _) = start_fake_server(ServerBehavior::default()).await;
    let config = SmtpConfig::new("127.0.0.1", port);
    let transport = SmtpTransport::new(config);

    let mut msg = test_message();
    msg.envelope = Some(trustmailer_core::Envelope {
        from: "sender@example.com".to_string(),
        to: vec!["victim@target.com>\r\nDATA\r\nphishing\r\n.\r\nQUIT".to_string()],
    });

    let err = transport.send(&msg).await.unwrap_err();
    assert!(err.to_string().contains("invalid recipient"));
}
