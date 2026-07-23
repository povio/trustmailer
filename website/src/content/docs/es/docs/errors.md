---
title: Errores
description: Cada variante de error de trustmailer, qué la dispara y cómo aparece en Rust y Node.js.
---

Nodemailer expone los fallos como un objeto `Error` con un `.code` de texto (`"EENVELOPE"`,
`"EAUTH"`, `"ECONNECTION"`, …) que comparas manualmente. trustmailer usa en su lugar un enum
tipado de Rust — `trustmailer_core::Error`, definido con
[`thiserror`](https://docs.rs/thiserror) — así que los fallos se pueden emparejar de forma
exhaustiva en tiempo de compilación, no con comparaciones de string que pueden desincronizarse
silenciosamente de la librería real.

```rust
pub enum Error {
    InvalidAddress(String),
    NoRecipients,
    NoSender,
    Attachment(String),
    Mime(String),
    Transport(String),
    Io(std::io::Error),
    Dkim(String),
}
```

Toda llamada falible de trustmailer — el `transport.send(&message)` que sigue a
`Message::builder()...build()`, `Attachment::resolve(...)`, `DkimSigner::from_pkcs8_pem(...)`
— devuelve `trustmailer::Result<T>` (un alias de `Result<T, Error>`), así que `?` y `match`
funcionan exactamente igual que con cualquier otro tipo de error de Rust.

## Variantes

### `InvalidAddress(String)`

Una cadena de dirección no pudo resolverse a nada utilizable. En la práctica esto es raro —
el [parser de direcciones](/es/docs/addresses/) es deliberadamente permisivo/infalible para
texto libre (el mismo diseño de nodemailer: mejor una suposición razonable que un fallo duro
ante un `To:` desordenado) — esta variante existe para los casos más estrechos donde una
`Address` estructurada suministrada por el llamador es inutilizable de todas formas (p. ej.
vacía tras recortar espacios).

### `NoRecipients`

Texto del mensaje: `"no recipients defined (to/cc/bcc/envelope.to)"`.

Lanzado por [`Message::effective_envelope()`](/es/docs/message/#envelope-y-effective_envelope)
cuando `to`, `cc`, `bcc` y una sobreescritura explícita de `envelope` están *todos* vacíos —
no hay a dónde enviar el `RCPT TO` de la transacción SMTP. Solución: define al menos uno de
`to`/`cc`/`bcc`, o llama a `.envelope(...)` directamente.

### `NoSender`

Texto del mensaje: `"no sender defined (from/sender/envelope.from)"`.

La contraparte simétrica de `NoRecipients` — se lanza cuando `from`, `sender` y una
sobreescritura explícita de `envelope` están todos vacíos, así que `MAIL FROM` no tiene
nada que enviar. Solución: llama a `.from(...)` (o `.envelope(...)`) antes de `.build()`.

:::note[Añadido en la segunda auditoría de seguridad]
Esta variante no existía en la primera pasada — antes un llamador podía obtener un sobre
con un `from` vacío sin ningún error. Ver [Seguridad](/es/docs/security/) para la historia
completa de lo que encontró y corrigió la re-auditoría independiente.
:::

### `Attachment(String)`

Envuelve todo lo que puede salir mal al resolver el contenido de un
[adjunto](/es/docs/attachments/) a bytes:

- `AttachmentContent::Base64` con contenido que no es base64 válido.
- `AttachmentContent::Url` cuando la petición HTTP en sí falla (DNS, conexión rechazada,
  error TLS, estado no-2xx).
- `AttachmentContent::Url` cuando la respuesta excede el límite de 25MB
  `MAX_FETCH_URL_BYTES` de trustmailer-core — comprobado contra el `Content-Length` de
  antemano *y* impuesto mientras se transmite, así que un servidor que mienta sobre su
  `Content-Length` no puede evadirlo.
- `AttachmentContent::Url` cuando leer el cuerpo de la respuesta falla a mitad de camino.

### `Mime(String)`

Poco común en la práctica hoy — la única fuente actual es el transporte de
`trustmailer-json` fallando al serializar un `Message` a JSON (un error interno de
`serde_json`, no algo que el input del llamador dispare normalmente). Reservado de forma
más amplia para fallos al momento de construir el MIME.

### `Transport(String)`

El comodín para fallos en la capa de entrega de cualquier transporte: errores de
conexión/handshake/auth/protocolo SMTP (ver la página de [transporte SMTP](/es/docs/transports/smtp/)
para las muchas condiciones específicas que caen aquí — STARTTLS rechazado, mecanismo AUTH
no anunciado, proxy SOCKS5 mal configurado, fallo al obtener el token OAuth2, timeouts de
conexión/comando), fallos al lanzar/salir del proceso sendmail, y respuestas no-2xx de la
API HTTP de SES. Si necesitas distinguir *qué* transporte falló, compara el texto de error
específico del transporte embebido en el `String` — no hay más sub-tipado hoy.

### `Io(std::io::Error)`

Envuelve un `std::io::Error` crudo vía `#[from]` — sobre todo I/O de sistema de archivos
para `AttachmentContent::Path` (tras la feature `fs`) y cualquier I/O de socket de más bajo
nivel que aparezca como `io::Error` en vez de envolverse primero en un mensaje `Transport`.

### `Dkim(String)`

Todo lo específico de DKIM, desde [`DkimSigner`](/es/docs/dkim/):

- `domain`/`selector` que contienen `\r`, `\n`, `;`, o `"` — rechazados en la construcción,
  antes de que se construya ningún header, cerrando la vía de inyección de headers que
  encontró la primera auditoría.
- Una clave privada PKCS#8 inválida pasada a `from_pkcs8_pem`.
- No se encuentran headers firmables en el mensaje.
- **El header `From` debe estar firmado** — DKIM lo exige según RFC 6376; si
  `.with_headers(...)` excluye `"from"` (o el mensaje no tiene `From` en absoluto), la
  firma se rechaza en vez de emitir una firma que no cubra la identidad del remitente.
- Que la propia operación de firma RSA falle (esencialmente nunca, salvo una clave
  corrupta).

## Comparando en Rust

```rust
use trustmailer::Error;

match transport.send(&message).await {
    Ok(info) => println!("enviado: {}", info.message_id),
    Err(Error::NoRecipients) => eprintln!("nada a quién enviar — revisa to/cc/bcc"),
    Err(Error::Transport(msg)) => eprintln!("falló la entrega: {msg}"),
    Err(e) => eprintln!("falló el envío: {e}"),
}
```

## En Node.js

`trustmailer-node` mapea cada variante de `Error` a un `napi::Error` con un prefijo
`[VariantName]` en el mensaje y un `Status` elegido por variante — verificado desde el
`to_napi_err` de `crates/trustmailer-node/src/lib.rs`:

| Variante | `Status` de napi | Prefijo del mensaje |
| --- | --- | --- |
| `InvalidAddress` | `InvalidArg` | `[InvalidAddress]` |
| `NoRecipients` | `InvalidArg` | `[NoRecipients]` |
| `NoSender` | `InvalidArg` | `[NoSender]` |
| `Attachment` | `InvalidArg` | `[Attachment]` |
| `Mime` | `GenericFailure` | `[Mime]` |
| `Transport` | `GenericFailure` | `[Transport]` |
| `Io` | `GenericFailure` | `[Io]` |
| `Dkim` | `InvalidArg` | `[Dkim]` |

```js
try {
  await transporter.sendMail({ subject: "Hello", text: "Hi!" }); // sin to/from
} catch (err) {
  if (err.message.startsWith("[NoRecipients]")) {
    console.error("define al menos uno de to/cc/bcc");
  }
  throw err;
}
```

:::tip[Sin códigos de error de texto que mantener sincronizados]
Los strings `err.code` de nodemailer (`"EENVELOPE"`, `"EAUTH"`, …) solo viven en su
documentación — no hay ninguna comprobación del compilador de que tu
`if (err.code === "EAUTH")` siga correspondiendo a un código real tras una actualización.
El `match` de Rust de trustmailer sobre `Error` es exhaustivo y verificado por el
compilador; el prefijo `[VariantName]` en Node.js es el equivalente más cercano de ese
lado, generado directamente desde el mismo enum en vez de mantenerse a mano por separado.
:::
