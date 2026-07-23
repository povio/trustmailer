//! Sendmail transport: pipes the composed RFC 5322 message into a local
//! sendmail-compatible binary's stdin (`/usr/sbin/sendmail -i -t` by default).
//! Native runtimes only — WASM sandboxes can't spawn processes.

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use trustmailer_core::{mime, Error, Message, Result, SentInfo, Transport};

pub struct SendmailTransport {
    path: String,
    args: Vec<String>,
}

impl SendmailTransport {
    /// Uses `/usr/sbin/sendmail -i -t` (recipients taken from the message headers).
    pub fn new() -> Self {
        Self {
            path: "/usr/sbin/sendmail".to_string(),
            args: vec!["-i".to_string(), "-t".to_string()],
        }
    }

    /// Path to the sendmail-compatible binary. This is operator-supplied transport
    /// configuration, not attacker-controllable input — it is never derived from mail
    /// content. It should generally be an absolute path; a relative value falls back to
    /// a `PATH` lookup, which is usually not what you want in production.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }
}

impl Default for SendmailTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for SendmailTransport {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let raw = mime::build(message, "localhost")?;
        let envelope = message.effective_envelope()?;

        let mut child = Command::new(&self.path)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| Error::Transport(format!("failed to spawn {}: {e}", self.path)))?;

        let mut stdin = child.stdin.take().expect("stdin was piped");
        let mut stdout = child.stdout.take().expect("stdout was piped");
        let mut stderr = child.stderr.take().expect("stderr was piped");

        // Write stdin and drain stdout/stderr *concurrently*. A sendmail binary (or a
        // verbose wrapper around it) can write enough to stdout/stderr to fill the OS
        // pipe buffer before it has consumed all of a large stdin body (e.g. a message
        // with big attachments) — writing stdin to completion first and only then
        // reading stdout/stderr (the previous approach, via `wait_with_output()`) can
        // deadlock both sides forever in that case, with no timeout.
        let write_stdin = async {
            let result = stdin.write_all(&raw).await;
            drop(stdin); // close stdin so the child sees EOF regardless of outcome
            result
        };
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let read_stdout = stdout.read_to_end(&mut stdout_buf);
        let read_stderr = stderr.read_to_end(&mut stderr_buf);

        let (write_result, stdout_result, stderr_result) =
            tokio::join!(write_stdin, read_stdout, read_stderr);
        write_result.map_err(|e| Error::Transport(format!("write to sendmail failed: {e}")))?;
        stdout_result.map_err(|e| Error::Transport(format!("read sendmail stdout failed: {e}")))?;
        stderr_result.map_err(|e| Error::Transport(format!("read sendmail stderr failed: {e}")))?;

        let status = child
            .wait()
            .await
            .map_err(|e| Error::Transport(format!("sendmail wait failed: {e}")))?;
        if !status.success() {
            let stderr_text = String::from_utf8_lossy(&stderr_buf);
            return Err(Error::Transport(format!(
                "sendmail exited with {status}: {stderr_text}"
            )));
        }

        Ok(SentInfo {
            message_id: message.message_id.clone().unwrap_or_default(),
            accepted: envelope.to.clone(),
            rejected: Vec::new(),
            envelope,
            response: Some(String::from_utf8_lossy(&stdout_buf).to_string()),
            raw: None,
        })
    }

    fn name(&self) -> &'static str {
        "sendmail"
    }
}

// Behavioral ports of test/sendmail/sendmail-test.js ("should send message" / "should
// return an error"): rather than mocking `child_process.spawn` like nodemailer does,
// these spawn real `/bin/cat` and `/bin/sh` processes as stand-ins for `sendmail` —
// closer to what actually happens in production, at the cost of being Unix-only.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use trustmailer_core::Message;

    fn test_message() -> Message {
        Message::builder()
            .from("a@example.com")
            .to("b@example.com")
            .subject("hi")
            .text("hello world")
            .build()
    }

    #[tokio::test]
    async fn pipes_the_composed_message_to_sendmails_stdin() {
        let transport = SendmailTransport::new()
            .with_path("/bin/cat")
            .with_args(vec![]);
        let info = transport.send(&test_message()).await.unwrap();
        let response = info.response.unwrap();
        assert!(response.contains("hello world"));
        assert!(response.contains("From: a@example.com"));
    }

    #[tokio::test]
    async fn envelope_recipients_come_from_the_message() {
        let transport = SendmailTransport::new()
            .with_path("/bin/cat")
            .with_args(vec![]);
        let info = transport.send(&test_message()).await.unwrap();
        assert_eq!(info.accepted, vec!["b@example.com".to_string()]);
    }

    #[tokio::test]
    async fn nonzero_exit_status_is_reported_as_an_error() {
        let transport = SendmailTransport::new()
            .with_path("/bin/sh")
            .with_args(vec!["-c".to_string(), "exit 42".to_string()]);
        let err = transport.send(&test_message()).await.unwrap_err();
        assert!(err.to_string().contains("42"));
    }

    #[tokio::test]
    async fn missing_binary_is_reported_as_an_error() {
        let transport = SendmailTransport::new().with_path("/no/such/sendmail-binary");
        assert!(transport.send(&test_message()).await.is_err());
    }
}
