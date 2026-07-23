---
title: Mensaje
description: Todos los campos de Message/MessageBuilder en trustmailer — contenido, enrutamiento, cabeceras y cómo se convierte en un flujo MIME.
---

Un `Message` es el equivalente en trustmailer al objeto de opciones `Mail` de
nodemailer: un único struct de Rust (`trustmailer_core::Message`) que reúne todo lo
relativo a un correo — remitente, destinatarios, cuerpo, adjuntos y cabeceras — antes de
entregarlo a un `Transport`. Normalmente no construyes un `Message` a mano, sino a
través de `Message::builder()`, un `MessageBuilder` encadenable que reutiliza los mismos
nombres de campo que nodemailer allí donde el concepto también existe en Rust. Ese mismo
modelo se expone desde Node.js como `MailOptions`, desde hosts WASM/navegador a través
del mismo crate `core`, y desde Cloudflare Workers vía `trustmailer-smtp-cfworkers` —
consulta [Arquitectura](/es/docs/architecture/) para ver cómo un único crate
`trustmailer-core` sostiene todos los runtimes.

```rust
use trustmailer::Message;

let message = Message::builder()
    .from("Alice <alice@example.com>")
    .to("bob@example.com")
    .cc(vec!["carol@example.com", "dave@example.com"])
    .subject("Hola")
    .text("¡Hola Bob!")
    .html("<b>¡Hola Bob!</b>")
    .build();
```

Desde Node.js, el mismo mensaje es un objeto plano que se pasa a `sendMail`:

```js
const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  cc: "carol@example.com,dave@example.com",
  subject: "Hola",
  text: "¡Hola Bob!",
  html: "<b>¡Hola Bob!</b>",
});
```

:::note[Los bindings de Node exponen un subconjunto]
`MailOptions` de `trustmailer-node` cubre hoy `from`, `to`, `cc`, `bcc`, `replyTo`,
`subject`, `text`, `html`, `messageId` y `attachments` (ver
`crates/trustmailer-node/src/lib.rs`). Los campos que aparecen más abajo en esta
página — `watchHtml`/`ampHtml`, cabeceras personalizadas, `priority`, `envelope`,
`icalEvent` y la familia `list`/`listUnsubscribe` — están implementados en
`trustmailer-core` y disponibles para cualquier llamador nativo de Rust o WASM, pero aún
no están conectados al binding de napi. Si necesitas alguno de ellos desde Node hoy
mismo, construye el MIME crudo directamente con el crate `core` en lugar de pasar por
`MailOptions`.
:::

## Campos comunes

| Campo | Método del builder | Tipo | Descripción |
|---|---|---|---|
| `from` | `.from(addr)` | dirección | Remitente mostrado en la cabecera `From:`. También el envelope-from por defecto (`MAIL FROM`) salvo que se sobrescriba — ver [Envelope](#envelope-y-effective_envelope) más abajo. |
| `to` | `.to(addrs)` | lista de direcciones | Destinatarios principales, cabecera `To:`. |
| `cc` | `.cc(addrs)` | lista de direcciones | Destinatarios en copia, cabecera `Cc:` — visibles para el resto de destinatarios. |
| `bcc` | `.bcc(addrs)` | lista de direcciones | Destinatarios en copia oculta. Se usan para construir el envelope SMTP pero **nunca** se escriben en ninguna cabecera — así lo confirma la propia batería de tests de trustmailer, que verifica que jamás aparece una línea `Bcc:` en la salida renderizada. |
| `subject` | `.subject(s)` | string | Cabecera `Subject:`. Se codifica automáticamente según RFC 2047 si contiene texto no ASCII. |
| `text` | `.text(s)` | string | Cuerpo en texto plano, renderizado como parte `text/plain; charset=utf-8`, codificado en quoted-printable. |
| `html` | `.html(s)` | string | Cuerpo HTML, renderizado como parte `text/html; charset=utf-8`. |
| `attachments` | `.attachment(a)` (repetible) | `Attachment` | Archivos e imágenes incrustadas — ver [Adjuntos](/es/docs/attachments/). |

`to`, `cc`, `bcc` y `reply_to` aceptan cualquier tipo que implemente `IntoAddresses`: un
único `&str`/`String` (interpretado como lista de direcciones separadas por coma o
punto y coma), un `Address`, un `Vec<Address>`, o un `Vec<&str>` (cada entrada se
interpreta por separado). Consulta [Direcciones](/es/docs/addresses/) para conocer las
reglas completas de parseo — nombres visibles entre comillas, comentarios usados como
nombre, grupos RFC 5322 y los topes de seguridad ante entradas patológicas.

## Campos de contenido

Además de `text`/`html`, trustmailer soporta los mismos tres canales de contenido
adicionales que nodemailer, cada uno renderizado como su propia alternativa MIME:

| Campo | Método del builder | Se renderiza como | Propósito |
|---|---|---|---|
| `watch_html` | `.watch_html(s)` | `text/watch-html; charset=utf-8` | Un HTML simplificado de respaldo para clientes que descartan el HTML completo pero toleran marcado ligero (históricamente Apple Watch). |
| `amp_html` | `.amp_html(s)` | `text/x-amp-html; charset=utf-8` | Un cuerpo interactivo AMP4EMAIL, para clientes que lo soporten. |
| `ical_event` | `.ical_event(event)` | `text/calendar; charset=utf-8; method=...` | Una invitación o actualización de calendario — ver [Invitaciones de calendario](#invitaciones-de-calendario) más abajo. |

Cuando hay más de una parte de contenido, se envuelven en un contenedor
`multipart/alternative` en un orden fijo: `text` → `watch_html` → `amp_html` → `html` →
cualquier `alternatives` extra → `ical_event`. Los clientes de correo eligen la última
alternativa que sepan renderizar, por eso la parte más rica (HTML, y luego la
invitación de calendario) va al final.

```rust
let message = Message::builder()
    .text("Respaldo en texto plano.")
    .watch_html("<b>Versión simplificada para pantallas pequeñas</b>")
    .html("<h1>HTML completo</h1><p>...</p>")
    .build();
```

:::tip[Sin ningún contenido, igualmente se envía]
Si no se define ni `text` ni `html` (ni ningún otro campo de contenido), trustmailer
recurre a una única parte `text/plain` vacía en lugar de producir un mensaje inválido —
útil para notificaciones que solo llevan adjuntos.
:::

### Invitaciones de calendario

`ical_event` recibe un `CalendarEvent`, construido a partir del contenido `.ics` crudo:

```rust
use trustmailer::message::{CalendarEvent, CalendarMethod};

let event = CalendarEvent::new(ics_body).with_method(CalendarMethod::Request);

let message = Message::builder()
    .to("bob@example.com")
    .subject("Invitación a reunión")
    .text("Ver invitación adjunta.")
    .ical_event(event)
    .build();
```

`CalendarMethod` es un enum de 4 valores (`Publish` — el valor por defecto, `Request`,
`Reply`, `Cancel`) que coincide exactamente con la propiedad `METHOD` de RFC 5546; es un
enum en lugar de un string suelto precisamente para que jamás pueda interpolarse un
valor arbitrario en el parámetro `Content-Type: text/calendar; method=...`. El nombre de
archivo por defecto de `CalendarEvent` es `invite.ics`.

## Enrutamiento y envelope

| Campo | Método del builder | Notas |
|---|---|---|
| `reply_to` | `.reply_to(addrs)` | Cabecera `Reply-To:` — a dónde deben ir las respuestas si difiere de `from`. |
| `envelope` | `.envelope(env)` | Envelope SMTP explícito (`MAIL FROM` / `RCPT TO`), que sobrescribe el que trustmailer derivaría por defecto. Ver más abajo. |
| `sender`, `in_reply_to`, `references`, `date` | *(sin método del builder todavía)* | Se asignan directamente sobre el `Message` ya construido — todos sus campos son `pub`. Ver [Campos sin método del builder](#campos-sin-método-del-builder-todavía). |

### Envelope y `effective_envelope()`

El envelope SMTP (a quién se entrega realmente el mensaje a nivel de protocolo) puede
diferir legítimamente de las cabeceras visibles `From`/`To` — por ejemplo, el manejo de
rebotes VERP usa un `MAIL FROM` que codifica el destinatario original. Llama a
`.envelope(...)` para fijarlo explícitamente; si no, `Message::effective_envelope()` lo
deriva igual que nodemailer:

- **from**: `sender` si está definido, si no `from`. Si ninguno está definido, devuelve
  `Error::NoSender`.
- **to**: `to` + `cc` + `bcc`, aplanados (cualquier grupo de direcciones se expande a
  sus miembros individuales, ya que un envelope SMTP no tiene noción de grupo),
  deduplicados (preservando el orden original — no se ordenan, porque el orden del
  envelope puede importarle a un servidor receptor), descartando las direcciones
  vacías. Si el resultado queda vacío, devuelve `Error::NoRecipients`.

```rust
use trustmailer::error::Error;

match message.effective_envelope() {
    Ok(envelope) => { /* envelope.from, envelope.to */ }
    Err(Error::NoSender) => eprintln!("el mensaje no tiene dirección from/sender"),
    Err(Error::NoRecipients) => eprintln!("el mensaje no tiene destinatarios to/cc/bcc"),
    Err(e) => eprintln!("inesperado: {e}"),
}
```

:::caution[Bcc llega al envelope, nunca a las cabeceras]
Los destinatarios en `bcc` se incluyen en `effective_envelope().to` (así que el mensaje
realmente se les entrega) pero nunca se escriben en ninguna cabecera renderizada — ese
es justamente el propósito de una copia oculta. No asumas que una dirección `bcc`
aparecerá en ningún sitio del MIME crudo.
:::

## Opciones de cabecera

| Campo | Método del builder | Notas |
|---|---|---|
| `message_id` | `.message_id(id)` | Valor explícito de `Message-ID:`. Si se omite, se genera automáticamente uno como `<uuid@hostname>` al construir el mensaje (o, vía el binding de Node, se pregenera antes de enviar para poder reportarlo en el resultado). |
| `priority` | `.priority(p)` | `Priority::High`/`Normal`/`Low`. Fija a la vez las cabeceras `X-Priority` (`1 (Highest)` / `3 (Normal)` / `5 (Lowest)`) e `Importance` (`high`/`normal`/`low`). |
| `headers` | `.header(name, value)` (repetible) | Cabecera adicional arbitraria, añadida tal cual (codificada según RFC 2047 automáticamente si no es ASCII). |
| `list` | `.list(key, entries)`, `.list_unsubscribe(entries)`, `.list_unsubscribe_post()` | Cabeceras `List-*` — ver más abajo. |

Todo valor de cabecera, sea cual sea su origen — subject, un `.header(...)`
personalizado, un comentario de `list`, una cabecera de adjunto — pasa por la misma
ruta de serialización (`mime/node.rs::write_header_line`), que colapsa cualquier `\r`/`\n`
embebido a un único espacio antes de escribir la cabecera. Un valor malicioso o mal
formado no puede terminar una cabecera antes de tiempo e inyectar una cabecera
falsificada a continuación (ver [Seguridad](/es/docs/security/)). Las líneas de cabecera
largas también se pliegan a ~76 columnas según RFC 5322 §2.2.3, partiendo únicamente en
comas que queden fuera de cadenas entre comillas, para que una coma dentro de un nombre
visible entrecomillado nunca se use como punto de plegado.

### Cabeceras `List-*`

`.list(key, entries)` fija una cabecera `List-<Key>` (`"unsubscribe"` → `List-Unsubscribe`,
`"id"` → `List-ID`, `"help"`, `"subscribe"`, `"post"`, `"owner"`, `"archive"`, o
cualquier otro sufijo que elijas — se acepta cualquier clave, igual que la opción
`list` de nodemailer). Cada entrada es un `ListEntry`: una URL más un comentario legible
opcional:

```rust
use trustmailer::message::ListEntry;

let message = Message::builder()
    .from("news@example.com")
    .to("subscriber@example.com")
    .subject("Boletín")
    .text("...")
    .list_unsubscribe(vec![
        ListEntry::new("https://example.com/unsub?id=123").with_comment("Un solo clic"),
        ListEntry::new("mailto:unsub@example.com"),
    ])
    .list_unsubscribe_post()
    .build();
```

`list_unsubscribe(entries)` es un atajo para `.list("unsubscribe", entries)`.
`list_unsubscribe_post()` añade `List-Unsubscribe-Post: List-Unsubscribe=One-Click` según
RFC 8058 — las reglas de remitentes masivos de Gmail y Yahoo exigen esto junto a una URL
de `List-Unsubscribe` funcional. Un valor `usuario@host` a secas se envuelve como
`<mailto:usuario@host>`; una URL `http(s)`/`ftp`/`mailto` se envuelve como
`<esquema:...>`; cualquier otra cosa recurre a `<http://...>`. `List-ID` recibe un trato
especial con la forma `<dominio>` a secas de RFC 2919, en lugar de `<esquema:url>`.

## Campos sin método del builder todavía

Todos los campos de `Message` son `pub`, y `MessageBuilder::build()` te entrega un
`Message` normal — así que unos pocos campos que aún no tienen un método encadenable
propio se pueden fijar directamente sobre el valor que devuelve `build()`:

```rust
let mut message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .text("...")
    .build();

message.sender = Some("alice-bounces@example.com".into()); // cabecera Sender:
message.in_reply_to = Some("<previous@example.com>".into()); // In-Reply-To:
message.references = vec!["<thread-root@example.com>".into()]; // References:
message.date = Some(time::OffsetDateTime::now_utc()); // sobrescribe el Date: automático
message.raw = Some(prebuilt_mime_bytes); // se salta el builder por completo — ver abajo
```

- **`sender`** — cabecera `Sender:`, distinta de `From:` (p. ej. alguien enviando en
  nombre de otra dirección). También se usa como envelope-from antes que `from`, si está
  definido.
- **`in_reply_to`** / **`references`** — cabeceras de hilo de conversación.
- **`date`** — sobrescribe la cabecera `Date:` autogenerada (formato RFC 5322, `now()`
  en UTC por defecto). Su tipo es `Option<time::OffsetDateTime>` del crate
  [`time`](https://docs.rs/time) — trustmailer no lo reexporta, así que definir este campo
  directamente requiere añadir también `time = "0.3"` a tu propio `Cargo.toml`.
- **`alternatives`** — partes MIME adicionales de nivel superior más allá de
  `text`/`html`/`watch_html`/`amp_html` (p. ej. una parte `text/calendar` construida por
  ti mismo en lugar de vía `ical_event`); se añaden empujando directamente sobre
  `message.alternatives`.
- **`raw`** — un flujo de bytes RFC 5322 ya completamente construido. Cuando está
  definido, el constructor MIME lo devuelve tal cual e ignora cualquier otro campo del
  mensaje — el equivalente en trustmailer a la opción `raw` de nodemailer, para quien ya
  tiene un mensaje completo listo para enviar sin más.

## Cómo se convierte en MIME

`build(&message, hostname)`, en el constructor MIME de `trustmailer-core`, ensambla las
partes anteriores en un flujo de bytes RFC 5322:

1. Cada campo de contenido presente se convierte en un nodo hoja, en el orden fijo
   descrito en [Campos de contenido](#campos-de-contenido). Una sola parte no necesita
   envoltorio; dos o más se envuelven en `multipart/alternative`.
2. Los adjuntos incrustados (`content_disposition: inline` **y** un `cid` definido) se
   envuelven junto con ese cuerpo en `multipart/related`, para que las referencias
   `<img src="cid:...">` se resuelvan dentro de la misma unidad que el cliente renderiza.
3. Cualquier adjunto restante (no incrustado, o incrustado sin `cid`) vuelve a envolver
   el resultado en `multipart/mixed`.
4. Las cabeceras raíz (`From`, `To`, `Cc`, `Reply-To`, `Subject`, `Date`, `Message-ID`,
   `List-*`, cabeceras personalizadas, ...) se anteponen al final. `Bcc` deliberadamente
   nunca está entre ellas.

Consulta [Adjuntos](/es/docs/attachments/) para la historia completa de adjuntos e
imágenes incrustadas, y [Direcciones](/es/docs/addresses/) para cómo se interpreta y
renderiza cada campo de dirección mencionado arriba.
