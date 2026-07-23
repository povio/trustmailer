use crate::config::SmtpConfig;
use crate::connection::SmtpConnection;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use trustmailer_core::{mime, Message, Result, SentInfo, Transport};

/// A pooled SMTP transport that reuses authenticated connections across sends,
/// bounded to `config.pool_size` concurrent connections — mirrors nodemailer's
/// `pool: true` SMTP transport option.
pub struct SmtpPool {
    config: SmtpConfig,
    idle: Arc<Mutex<Vec<SmtpConnection>>>,
    permits: Arc<Semaphore>,
}

impl SmtpPool {
    pub fn new(config: SmtpConfig) -> Self {
        let permits = Arc::new(Semaphore::new(config.pool_size.max(1)));
        Self {
            config,
            idle: Arc::new(Mutex::new(Vec::new())),
            permits,
        }
    }

    async fn checkout(&self) -> Result<(SmtpConnection, tokio::sync::OwnedSemaphorePermit)> {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| trustmailer_core::Error::Transport(format!("pool closed: {e}")))?;

        // Idle pooled connections may have been closed server-side (SMTP servers
        // commonly drop connections after an idle timeout) — probe with a NOOP before
        // handing one back to the caller, so a stale connection surfaces as a
        // transparent reconnect instead of a raw I/O error on the caller's first real
        // command. Keep trying idle connections until one is live, or fall back to
        // dialing a fresh one.
        loop {
            let existing = self.idle.lock().await.pop();
            match existing {
                Some(mut conn) => {
                    if conn.noop().await.is_ok() {
                        return Ok((conn, permit));
                    }
                    // Stale/dead connection: drop it (closing the underlying socket)
                    // and try the next idle connection, or dial fresh once idle is empty.
                }
                None => {
                    let conn = SmtpConnection::connect(&self.config).await?;
                    return Ok((conn, permit));
                }
            }
        }
    }

    async fn checkin(&self, conn: SmtpConnection) {
        let mut idle = self.idle.lock().await;
        if idle.len() < self.config.pool_size {
            idle.push(conn);
        }
        // else: drop the connection, closing it (pool is already at capacity of idle conns).
    }

    /// Closes all currently idle connections. In-flight sends are unaffected.
    pub async fn close(&self) {
        // Drain the idle list under the lock, then drop it before doing any network
        // I/O — `QUIT` round trips one connection at a time would otherwise hold the
        // lock for as long as all of them take combined, blocking concurrent
        // checkout/checkin in the meantime.
        let conns: Vec<SmtpConnection> = {
            let mut idle = self.idle.lock().await;
            idle.drain(..).collect()
        };
        for conn in conns {
            let _ = conn.quit().await;
        }
    }
}

#[async_trait]
impl Transport for SmtpPool {
    async fn send(&self, message: &Message) -> Result<SentInfo> {
        let raw = mime::build(message, &self.config.ehlo_name)?;
        let envelope = message.effective_envelope()?;

        let (mut conn, permit) = self.checkout().await?;
        let result = conn
            .send_transaction(&envelope.from, &envelope.to, &raw)
            .await;

        // Keep the permit held until the connection is actually back in the idle list
        // (or torn down on the error path) — dropping it earlier lets a new checkout
        // start dialing a fresh connection concurrently, transiently exceeding
        // `pool_size` connections in flight.
        let outcome = match result {
            Ok(response) => {
                self.checkin(conn).await;
                Ok(SentInfo {
                    message_id: message.message_id.clone().unwrap_or_default(),
                    accepted: envelope.to.clone(),
                    rejected: Vec::new(),
                    envelope,
                    response: Some(response),
                    raw: None,
                })
            }
            Err(e) => {
                // The connection is in an unknown state after a failed transaction; drop it
                // rather than returning it to the pool.
                let _ = conn.quit().await;
                Err(e)
            }
        };
        drop(permit);
        outcome
    }

    fn name(&self) -> &'static str {
        "smtp-pool"
    }
}
