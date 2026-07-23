//! Low-level byte-stream wrapper around a `cloudflare:sockets` `Socket`: line-buffered
//! reads and raw writes, driven through the Web Streams reader/writer protocol via
//! `wasm-bindgen-futures`. `cloudflare:sockets` is a workerd built-in module — this
//! only resolves at runtime inside an actual Cloudflare Worker, not in a browser, Deno,
//! Bun, or `wasm-pack test`.

use js_sys::{Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "cloudflare:sockets")]
extern "C" {
    #[wasm_bindgen(js_name = connect, catch)]
    fn cf_connect(address: &str, options: &JsValue) -> Result<JsValue, JsValue>;
}

fn get(obj: &JsValue, name: &str) -> Result<JsValue, JsValue> {
    Reflect::get(obj, &JsValue::from_str(name))
}

fn call0(obj: &JsValue, method: &str) -> Result<JsValue, JsValue> {
    let f: Function = get(obj, method)?.dyn_into()?;
    f.call0(obj)
}

fn call1(obj: &JsValue, method: &str, arg: &JsValue) -> Result<JsValue, JsValue> {
    let f: Function = get(obj, method)?.dyn_into()?;
    f.call1(obj, arg)
}

async fn await_promise(value: JsValue) -> Result<JsValue, JsValue> {
    let promise: Promise = value.dyn_into()?;
    JsFuture::from(promise).await
}

/// `secureTransport` value passed to `connect()`: `"on"` for implicit TLS (port 465
/// style), `"starttls"` to negotiate STARTTLS ourselves and upgrade via [`CfSocket::start_tls`].
pub enum SecureTransport {
    On,
    StartTls,
    /// No TLS at all, ever. Only meant for testing against a trusted plaintext server —
    /// see [`crate::CfSmtpConfig::with_require_tls`].
    Off,
}

impl SecureTransport {
    fn as_str(&self) -> &'static str {
        match self {
            SecureTransport::On => "on",
            SecureTransport::StartTls => "starttls",
            SecureTransport::Off => "off",
        }
    }
}

/// Generous upper bound on a single buffered SMTP response line. A few KB is far more
/// than any real server line (RFC 5321 caps reply lines at 512 octets); this only exists
/// to fail fast with a clear error instead of growing `buf` without bound if a server
/// (or a misbehaving proxy in front of one) never sends a line terminator — an OOM/isolate
/// -crash vector in a memory-capped Worker otherwise.
const MAX_LINE_BUF: usize = 8192;

/// A connected socket, buffering partial lines across reads so callers can `read_line`
/// against an SMTP server that doesn't align responses to the underlying stream's
/// chunk boundaries.
///
/// Note on cleanup: there is deliberately no `Drop` impl closing `socket` here. Real
/// cleanup (`close()`) is `async` — calling into JS to close the underlying
/// `cloudflare:sockets` socket — and Rust's `Drop` is synchronous, so it cannot await a
/// promise. A `Drop` impl could at best fire-and-forget a `close` call synchronously
/// (no `await`), which `wasm-bindgen`'s object model doesn't support without already
/// holding a `JsFuture` executor context, and would silently swallow any failure with no
/// way to log or propagate it. Instead, every call site that owns a `CfSocket` is
/// responsible for calling `close()` on all paths (success and error) — see
/// `lib.rs`'s `with_socket_cleanup` helper, which is the actual fix for the leak this
/// would otherwise cause.
pub struct CfSocket {
    socket: JsValue,
    reader: JsValue,
    writer: JsValue,
    buf: Vec<u8>,
}

impl CfSocket {
    pub async fn connect(
        host: &str,
        port: u16,
        secure_transport: SecureTransport,
    ) -> Result<Self, JsValue> {
        let address = format!("{host}:{port}");
        let options = Object::new();
        Reflect::set(
            &options,
            &"secureTransport".into(),
            &secure_transport.as_str().into(),
        )?;

        let socket = cf_connect(&address, &options)?;
        let opened = get(&socket, "opened")?;
        await_promise(opened).await?;

        Self::wrap(socket)
    }

    fn wrap(socket: JsValue) -> Result<Self, JsValue> {
        let readable = get(&socket, "readable")?;
        let reader = call0(&readable, "getReader")?;
        let writable = get(&socket, "writable")?;
        let writer = call0(&writable, "getWriter")?;
        Ok(Self {
            socket,
            reader,
            writer,
            buf: Vec::new(),
        })
    }

    /// Upgrades a `secureTransport: "starttls"` socket to TLS after the server has
    /// replied `220` to a `STARTTLS` command. The original socket is discarded per the
    /// `cloudflare:sockets` contract — every subsequent read/write goes through the
    /// upgraded reader/writer instead.
    ///
    /// Takes `&mut self` (rather than consuming `self`) so that on failure the caller
    /// still holds a valid, still-open (plaintext) socket it can close — consuming
    /// `self` here would drop the socket on error with no way for the caller to clean
    /// it up.
    pub async fn start_tls(&mut self) -> Result<(), JsValue> {
        let secure = call0(&self.socket, "startTls")?;
        let upgraded = Self::wrap(secure)?;
        *self = upgraded;
        Ok(())
    }

    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), JsValue> {
        let array = Uint8Array::from(data);
        let write_promise = call1(&self.writer, "write", &array.into())?;
        await_promise(write_promise).await?;
        Ok(())
    }

    pub async fn write_line(&mut self, line: &str) -> Result<(), JsValue> {
        let mut bytes = line.as_bytes().to_vec();
        bytes.extend_from_slice(b"\r\n");
        self.write_raw(&bytes).await
    }

    /// Reads one line (without the trailing CR/LF), pulling more chunks from the
    /// underlying stream as needed and buffering any bytes read past the line end.
    pub async fn read_line(&mut self) -> Result<String, JsValue> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                line.pop(); // trailing \n
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }

            let read_promise = call0(&self.reader, "read")?;
            let result = await_promise(read_promise).await?;
            let done = Reflect::get(&result, &"done".into())?
                .as_bool()
                .unwrap_or(true);
            if done {
                return Err(JsValue::from_str("connection closed unexpectedly"));
            }
            let value = Reflect::get(&result, &"value".into())?;
            let chunk: Uint8Array = value.dyn_into()?;
            let start = self.buf.len();
            let new_len = start + chunk.length() as usize;
            if new_len > MAX_LINE_BUF {
                return Err(JsValue::from_str(
                    "SMTP response line too long (exceeded 8192 bytes without a terminator)",
                ));
            }
            self.buf.resize(new_len, 0);
            chunk.copy_to(&mut self.buf[start..]);
        }
    }

    pub async fn close(&self) -> Result<(), JsValue> {
        let _ = call0(&self.socket, "close");
        Ok(())
    }
}
