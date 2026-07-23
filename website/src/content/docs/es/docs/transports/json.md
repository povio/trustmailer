---
title: Transporte JSON
description: Serializar un mensaje a JSON estructurado para pruebas — no un sustituto para probar la seguridad frente a inyección de headers.
---

El transporte JSON serializa un mensaje en un pequeño objeto JSON en lugar de construir
MIME RFC 5322 o entregarlo a ningún sitio. Refleja el `jsonTransport` de nodemailer, y
apunta directamente a pruebas y a runtimes donde prefieras entregar el mensaje a otra
cosa (tu propio relay HTTP, una vista previa en el navegador, una cola) en vez de que
trustmailer lo formatee como MIME. Está implementado en `trustmailer-json`, reexportado
desde el crate fachada como `trustmailer::json` tras la feature de Cargo `json`
(activada por defecto). Igual que el transporte stream, no hace ninguna E/S y está
disponible en todos los runtimes, incluido `trustmailer-wasm`.

## Configuración

`JsonTransport` no recibe ninguna configuración — `JsonTransport::new()` (o su
implementación de `Default`) es toda la superficie. No hay un equivalente al `path`/
`args` del transporte sendmail ni al `hostname` del transporte stream.

## Qué se serializa

`JsonTransport::render()` convierte el `Message` en un `JsonMessage`, y luego lo
serializa con `serde_json`:

| Campo         | Tipo                     | Notas |
| ------------- | ------------------------ | ----- |
| `from`        | `Address \| null`         | `{ "name": ..., "address": ... }`, o `null` si no está definido. |
| `to`          | `Address[]`               | |
| `cc`          | `Address[]`               | |
| `bcc`         | `Address[]`               | |
| `subject`     | `string \| null`          | |
| `text`        | `string \| null`          | |
| `html`        | `string \| null`          | |
| `attachments` | `Attachment[]`            | Cada entrada es `{ "filename": ..., "content_type": ... }` — el tipo de contenido resuelto (adivinado a partir de la extensión del archivo cuando no es explícito), no los bytes crudos del adjunto. |

Una `Address` se serializa como `{ "name": "...", "address": "..." }`, con `name` como
cadena vacía cuando la entrada original no tenía nombre de visualización (por ejemplo,
un simple `andris@nodemailer.com` en una lista de destinatarios junto a `Andris Kreata
<andris@kreata.ee>` — ambos se analizan, y el que va a secas obtiene `"name": ""`).

:::note[Deliberadamente más simple que el transporte JSON de nodemailer]
Este esquema no incluye el contenido de los adjuntos en línea, las cabeceras crudas ni
un subobjeto `envelope` como sí hace el transporte JSON de nodemailer — es una vista
estructural compacta del mensaje, no una serialización byte a byte de todo lo que
emite la versión de nodemailer. Si tus pruebas necesitan verificar bytes de adjuntos o
cabeceras generadas, usa el [transporte stream](/es/docs/transports/stream/) e
inspecciona `raw` en su lugar.
:::

## Forma de `SentInfo`

- `raw` — siempre `Some(bytes)`: los bytes UTF-8 de la cadena JSON de arriba.
- `response` — siempre la cadena fija `"250 json transport: message rendered, not
  sent"`, no el payload JSON en sí — lee `raw` (o llama a `render()` directamente) para
  obtener el JSON.
- `accepted` / `envelope` — se calculan a partir de `message.effective_envelope()`,
  igual que en cualquier otro transporte.
- `rejected` — siempre vacío.
- `message_id` — el propio campo `message_id` del `Message` si definiste uno, `""` en
  caso contrario (este transporte no coloca un Message-ID generado en ningún lugar de
  su salida JSON, a diferencia de los transportes basados en MIME, que siempre
  estampan uno en las cabeceras renderizadas incluso cuando el propio
  `SentInfo.message_id` está vacío).

## No es un sustituto para probar la inyección de cabeceras

:::caution[No ejercita el camino de saneamiento de CRLF del MIME]
El propio módulo de pruebas del transporte JSON lo dice explícitamente: serializa los
campos de dirección/asunto/cuerpo directamente como cadenas JSON, lo cual es un camino
de código completamente distinto de `mime::build()` — la función que comparten todos
los transportes reales (SMTP, sendmail, SES) y el transporte stream, y el único lugar
donde realmente vive el saneamiento contra inyección de cabeceras
(`mime/node.rs::write_header_line`, ver [Seguridad](/es/docs/security/)). Un asunto o
dirección que contenga un CRLF simplemente hará un round-trip como cadena JSON aquí; no
te dice nada sobre si esa misma entrada se sanearía de forma segura al enviarse de
verdad. Si estás escribiendo una prueba de regresión para inyección de cabeceras o
direcciones, usa el [transporte stream](/es/docs/transports/stream/) (o un transporte
real) y verifica sobre `raw`, no sobre este.
:::

## Ejemplo en Rust

```rust
use trustmailer::{Attachment, Message};
use trustmailer::json::JsonTransport;

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let transport = JsonTransport::new();

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("Bob <bob@example.com>")
        .subject("Hello")
        .text("Hi Bob!")
        .attachment(
            Attachment::from_bytes("report.pdf", std::fs::read("report.pdf")?)
                .with_content_type("application/pdf"),
        )
        .build();

    let json = transport.render(&message)?;
    println!("{json}");
    // {"from":{"name":"Alice","address":"alice@example.com"},"to":[...],...}
    Ok(())
}
```

Igual que en el transporte stream, `render()` es la herramienta directa para pruebas;
`Transport::send()` la envuelve con el cálculo del sobre y la cadena fija de
`SentInfo.response` descrita arriba.

## Node.js, Bun, Deno

`createTransport()` de `trustmailer-node` expone este transporte con
`jsonTransport: true`.

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({ jsonTransport: true });

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});
console.log(info.response); // "250 json transport: message rendered, not sent"
```

:::caution[Los bindings de Node/WASM no devuelven aún el JSON renderizado]
Misma salvedad que en el transporte stream: ni `SentInfoJs` (Node) ni su equivalente en
WASM exponen un campo `raw`, y `JsonTransport::send()` siempre fija `response` a la
cadena de confirmación fija en lugar del payload JSON — así que hoy, llamar a este
transporte desde Node.js, Bun, Deno o un host WASM te da metadatos de entrega, no el
documento JSON en sí. Llama a `render()` desde Rust directamente si necesitas el JSON.
:::

## Ver también

- [Transporte Stream](/es/docs/transports/stream/) — renderizado MIME crudo, y el
  transporte al que recurrir si específicamente necesitas probar la seguridad frente a
  inyección de cabeceras.
- [Transporte Sendmail](/es/docs/transports/sendmail/) y
  [Transporte SES](/es/docs/transports/ses/) — los transportes de entrega real para los
  que estos transportes de prueba son un sustituto.
- [Seguridad](/es/docs/security/) — dónde y cómo se previene realmente la inyección de
  cabeceras.
- [Matriz de soporte por runtime](/es/docs/runtime-matrix/).
