---
title: Transporte Stream
description: Renderizar un mensaje a bytes MIME crudos sin entregarlo a ningún sitio — para pruebas, relays propios y entrega hecha a mano.
---

El transporte stream renderiza un mensaje al mismo búfer de bytes RFC 5322 crudo que
construye cualquier transporte real (vía el `mime::build()` compartido), pero nunca lo
envía a ningún sitio. Refleja el `streamTransport` de nodemailer, y existe para tres
casos de uso que se solapan: verificar en pruebas los bytes exactos que produciría un
mensaje, entregar los bytes renderizados a tu propio mecanismo de entrega (un relay
propio, una cola, una llamada HTTP que hagas tú mismo), o conseguir que el correo
funcione de extremo a extremo en un runtime que no tiene ningún otro transporte de
entrega disponible. Está implementado en `trustmailer-stream`, reexportado desde el
crate fachada como `trustmailer::stream` tras la feature de Cargo `stream` (activada
por defecto). No hace ninguna E/S, así que está disponible en todos los runtimes,
incluido `trustmailer-wasm`.

## Configuración

| Campo      | Tipo     | Por defecto     | Descripción |
| ---------- | -------- | ---------------- | ----------- |
| `hostname` | `String` | `"localhost"`     | Hostname usado solo para generar un `Message-ID` cuando un mensaje aún no tiene uno definido. |

A diferencia del transporte sendmail (que fija `"localhost"` sin forma de cambiarlo),
`StreamTransport` te permite configurar este hostname — pero fíjate en la forma del
constructor: `StreamTransport::with_hostname(hostname)` es un constructor alternativo,
**no** un método builder encadenable sobre una instancia existente. Usa
`StreamTransport::new()` (hostname `"localhost"`) o
`StreamTransport::with_hostname("mail.example.com")` — no ambos juntos.

## `render()` frente a `send()`

`StreamTransport` expone un método `render(&self, message: &Message) ->
Result<Vec<u8>>` plano y síncrono, además del método `Transport::send()` del trait.
`render()` es la herramienta más directa para pruebas e inspección — simplemente
devuelve los bytes, sin cálculo de sobre ni envoltura en `SentInfo`. `send()` llama a
`render()` internamente y además calcula el sobre efectivo (`From`/`To`/`Cc`/`Bcc`
resueltos), así que usa `send()` cuando quieras específicamente un resultado con forma
de `Transport` (por ejemplo, para ejecutar exactamente el mismo camino de código que
usarías con SMTP/sendmail/SES en una prueba, cambiando solo el transporte).

## Forma de `SentInfo`

- `raw` — siempre `Some(bytes)`: el mensaje RFC 5322 completo renderizado, cabeceras y
  cuerpo, tal cual lo habría enviado cualquier transporte real. Este es el campo a
  inspeccionar en pruebas — verifica sobre `info.raw` para comprobar cabeceras,
  codificación, límites de adjuntos, etc.
- `response` — siempre la cadena fija `"250 stream transport: message rendered, not
  sent"`. No es el mensaje renderizado; no la analices buscando contenido MIME — lee
  `raw` en su lugar.
- `accepted` / `envelope` — se calculan igual que en cualquier otro transporte, a partir
  de `message.effective_envelope()`, así que la lógica de resolución de destinatarios
  se puede probar de la misma manera se entregue o no el correo realmente.
- `rejected` — siempre vacío; no hay nada que rechazar porque no se envió nada.
- `message_id` — el propio campo `message_id` del `Message` si definiste uno, `""` en
  caso contrario. Igual que en el transporte sendmail, el MIME *renderizado* en `raw`
  siempre tiene una cabecera `Message-ID` generada (usando `hostname`) incluso cuando el
  `Message` no tenía ninguna — pero `SentInfo.message_id` solo refleja lo que había en
  la estructura `Message` de entrada, no lo que terminó en los bytes renderizados.
- Un `Message` con `msg.raw` ya definido (ver `Message::builder()` / asignar `.raw`
  manualmente) pasa esa sobreescritura directamente byte a byte, sin modificar,
  saltándose `mime::build()` por completo.

## Ejemplo en Rust

```rust
use trustmailer::{Message, Transport};
use trustmailer::stream::StreamTransport;

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let transport = StreamTransport::with_hostname("example.com");

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .html("<b>Hi Bob!</b>")
        .build();

    let info = transport.send(&message).await?;
    let rendered = info.raw.expect("stream transport always sets raw");
    println!("{}", String::from_utf8_lossy(&rendered));
    Ok(())
}
```

Una prueba que solo necesite los bytes, sin manejo de sobre, puede llamar a `render()`
directamente en su lugar:

```rust
let transport = StreamTransport::new();
let bytes = transport.render(&message)?;
assert!(String::from_utf8_lossy(&bytes).contains("Hi Bob!"));
```

## Node.js, Bun, Deno

`createTransport()` de `trustmailer-node` expone este transporte con
`streamTransport: true`. Actualmente no hay forma de definir un `hostname`
personalizado desde el binding de Node — ese constructor solo existe en el
`StreamTransport` de Rust.

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({ streamTransport: true });

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Runtime check",
  text: "hello",
});
console.log(info.response); // "250 stream transport: message rendered, not sent"
```

:::caution[Los bindings de Node/WASM no devuelven aún los bytes renderizados]
Aquí importa leer con atención el código fuente del binding: `StreamTransport::send()`
fija `SentInfo.response` a esa cadena de confirmación fija y coloca los bytes
realmente renderizados en `SentInfo.raw` — y ni el `SentInfoJs` de `trustmailer-node`
ni el tipo de resultado equivalente en `trustmailer-wasm` exponen actualmente un campo
`raw`. Así que hoy, llamar a este transporte desde Node.js, Bun, Deno o un host WASM te
da metadatos de entrega (`accepted`, `rejected`, `messageId`) y esa cadena fija — no el
MIME renderizado. Si necesitas los bytes reales en JS, llama a `render()` desde Rust en
un módulo nativo, o usa el [transporte JSON](/es/docs/transports/json/) con la misma
salvedad en mente, o maneja `trustmailer` directamente desde Rust nativo.
:::

## Ver también

- [Transporte JSON](/es/docs/transports/json/) — una renderización estructurada
  alternativa, también para pruebas.
- [Transporte Sendmail](/es/docs/transports/sendmail/) y
  [Transporte SES](/es/docs/transports/ses/) — los transportes de entrega real para
  los que este es un sustituto.
- [Seguridad](/es/docs/security/) — el mismo camino de saneamiento de CRLF en cabeceras
  que este transporte ejercita, ya que comparte `mime::build()`.
- [Matriz de soporte por runtime](/es/docs/runtime-matrix/).
