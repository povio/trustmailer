---
title: Adjuntos
description: Adjuntar archivos e incrustar imágenes con trustmailer — fuentes de contenido, nombres de archivo y la advertencia de SSRF al descargar por URL.
---

Los adjuntos son archivos que viajan dentro de un [Message](/es/docs/message/), cada uno
un `trustmailer_core::Attachment` — reflejando el array `attachments` de nodemailer. Un
adjunto puede ser bytes que ya tienes en memoria, un string en base64, una ruta en
disco, o una URL remota descargada en el momento del envío; el mismo struct también
cubre las **imágenes incrustadas** (contenido inline referenciado desde HTML vía
`cid:`) mediante un campo adicional.

```rust
use trustmailer::{Message, attachment::Attachment};

let message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .subject("Informe adjunto")
    .text("Ver adjunto.")
    .attachment(Attachment::from_bytes("informe.pdf", pdf_bytes).with_content_type("application/pdf"))
    .build();
```

Llama a `.attachment(...)` una vez por archivo — añade a la lista, así que varias
llamadas (o un bucle) adjuntan varios archivos al mismo mensaje.

## Campos de un adjunto

| Campo | Tipo | Descripción |
|---|---|---|
| `filename` | `Option<String>` | Nombre mostrado al destinatario, y usado para detectar automáticamente un tipo MIME si no se define `content_type`. `None` omite el parámetro de nombre de archivo por completo. |
| `content` | `AttachmentContent` | De dónde vienen los bytes — ver [Fuentes de contenido](#fuentes-de-contenido). |
| `content_type` | `Option<String>` | Tipo MIME explícito. Si no se define, se adivina a partir de la extensión de `filename`, recurriendo a `application/octet-stream`. |
| `content_disposition` | `ContentDisposition` | `Attachment` (por defecto, descargable) o `Inline` (renderizado en el propio cuerpo — usado para imágenes incrustadas). |
| `cid` | `Option<String>` | Content-ID para referenciar este adjunto desde HTML como `cid:<valor>`. Se fija junto con una disposición `Inline` mediante `.inline_with_cid(...)`. |
| `headers` | `Vec<(String, String)>` | Cabeceras MIME adicionales en la propia parte de este adjunto, añadidas tal cual. |

## Fuentes de contenido

`AttachmentContent` es un enum con cuatro variantes, que coinciden con las fuentes
desde las que nodemailer permite obtener los bytes de un adjunto:

| Variante | Disponibilidad | Notas |
|---|---|---|
| `Bytes(Vec<u8>)` | En todas partes, incluido WASM | Bytes que ya tienes en memoria. |
| `Base64(String)` | En todas partes | Un payload codificado en base64, decodificado al resolver el adjunto — equivalente a la opción `encoding: "base64"` de nodemailer. |
| `Path(PathBuf)` | Solo nativo, tras el feature de Cargo `fs` | Se lee del disco local con `std::fs::read`. No hay sistema de archivos en un host WASM, así que esta variante ni siquiera existe ahí cuando `fs` está deshabilitado. |
| `Url(String)` | Tras el feature de Cargo `http` | Una URL remota, descargada por HTTP(S) — ver [Descargar desde una URL](#descargar-desde-una-url) más abajo. |

```rust
use trustmailer::attachment::{Attachment, AttachmentContent};

// Desde bytes ya en memoria:
let a1 = Attachment::from_bytes("datos.csv", csv_bytes);

// Desde un archivo local (nativo + feature `fs`):
let a2 = Attachment {
    content: AttachmentContent::Path("/tmp/informe.pdf".into()),
    ..Attachment::from_bytes("informe.pdf", Vec::new())
};
```

Llamar a `attachment.resolve_bytes()` sobre una variante `Url` que aún no se ha
descargado devuelve un error indicando que hay que llamar antes a `resolve_all()` —
nunca bloquea de forma síncrona para descargar por red.

### Detección del tipo de contenido

`resolved_content_type()` devuelve, en este orden: el `content_type` explícito si está
definido; si no, una estimación a partir de la extensión de `filename` (vía el crate
`mime_guess`); si no, `application/octet-stream`.

### Codificación de transferencia

Cada parte de adjunto se codifica en base64, sea cual sea su tipo de contenido, para
que el destinatario reciba el archivo byte a byte. Quoted-printable no se usa para
adjuntos, porque reescribe los saltos de línea LF como CRLF y no puede transportar
archivos `text/*` que no sean UTF-8 válido. De momento no hay ningún campo para forzar
otra codificación de transferencia.

## Nombres de archivo con unicode o caracteres especiales

El parámetro MIME `filename=`/`name=` se renderiza de dos formas distintas según lo
que contenga el nombre de archivo:

- Un nombre de archivo **ASCII plano** sin `"` literal ni caracteres de control se
  emite como una cadena entrecomillada simple: `filename="informe.pdf"`.
- Cualquier otra cosa — texto no ASCII (`"ÕÄÖÜ.txt"`), o ASCII que contenga un `"`
  literal que no se pueda incrustar de forma segura en una cadena entrecomillada
  (`"\"test\" it is.txt"`) — se emite en la forma extendida de RFC 2231:
  `filename*=UTF-8''%C3%95%C3%84...`.

Tanto el parámetro `name=` de `Content-Type` como el `filename=` de
`Content-Disposition` usan la misma regla.

:::note[Sin fragmentación en `name*0*=`]
A diferencia de nodemailer, trustmailer no divide un valor RFC 2231 muy largo en
fragmentos de continuación `name*0*=`/`name*1*=` — todo cliente de correo habitual
interpreta bien un valor `name*=UTF-8''...` en un solo bloque, y la fragmentación solo
importa de verdad para nombres de archivo de más de unos 100 caracteres.
:::

## Imágenes incrustadas (`cid`)

Para mostrar una imagen dentro del HTML en lugar de como un adjunto descargable,
adjúntala con `.inline_with_cid(id)`, que fija `content_disposition: Inline` y el `cid`
dado en una sola llamada, y luego referéncialo desde el cuerpo HTML como `cid:<id>`:

```rust
let message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .subject("Foto")
    .html(r#"<p>Mira:</p><img src="cid:logo123">"#)
    .attachment(
        Attachment::from_bytes("logo.png", logo_bytes)
            .with_content_type("image/png")
            .inline_with_cid("logo123"),
    )
    .build();
```

Adjunta tantas imágenes incrustadas como quieras, cada una con su propio `cid` único
que coincida con una referencia `<img src="cid:...">` en el HTML.

:::tip[Solo se agrupan así los adjuntos que son inline *y* tienen un `cid`]
Al construir el árbol MIME, trustmailer separa los adjuntos entre los que tienen
`content_disposition: Inline` **y** un `cid` definido, frente a todos los demás. Solo el
primer grupo se envuelve junto con el cuerpo del mensaje en un contenedor
`multipart/related` (para que los clientes de correo sepan resolver las referencias
`cid:` contra ellos); todo lo demás se envuelve por separado en `multipart/mixed` como
adjuntos descargables normales. Un adjunto marcado como `Inline` sin `cid` — o
cualquier adjunto cuando el mensaje no tiene cuerpo HTML que lo referencie — no se
incluye en `multipart/related`.
:::

Dale a un `cid` un valor con forma de dominio y resistente a colisiones (p. ej.
`logo@example.com`) en lugar de algo genérico como `img1` si estás construyendo
mensajes de forma programática a gran escala — nada te garantiza la unicidad por ti.

## Descargar desde una URL

`Attachment::from_url(filename, url)` (tras el feature de Cargo `http`) difiere la
descarga hasta que llamas a `resolve_all(&mut message)`, una función asíncrona que
recorre cada adjunto `Url` (y cada entrada de `alternatives`) del mensaje y lo
reemplaza por los bytes descargados en el sitio — llámala una vez, antes de
`build()`/`send()`:

```rust
use trustmailer::attachment::{resolve_all, Attachment};

let mut message = Message::builder()
    .from("alice@example.com")
    .to("bob@example.com")
    .subject("Logo")
    .text("Ver adjunto.")
    .attachment(Attachment::from_url("logo.png", "https://example.com/logo.png"))
    .build();

resolve_all(&mut message).await?;
let info = transport.send(&message).await?;
```

`resolve_all` también funciona desde un host navegador/WASM — en `wasm32`, el cliente
HTTP subyacente pasa por `fetch` en lugar de un socket crudo.

:::caution[Tope de 25MB, y sin protección SSRF]
Las respuestas descargadas están acotadas a **25MB** (`MAX_FETCH_URL_BYTES`) —
comprobado contra `Content-Length` de antemano cuando el servidor lo envía, y reforzado
de nuevo mientras se transmite el cuerpo en fragmentos, de modo que una respuesta con un
`Content-Length` ausente o falso tampoco puede superar el tope. Una respuesta que exceda
el límite, o un estado que no sea 2xx, hace fallar `resolve_all` con un error en lugar
de truncar en silencio.

Este tope de tamaño es la *única* protección que aplica `resolve_all`. **No** realiza
ninguna protección contra SSRF — no hay bloqueo de IPs privadas/link-local, ni lista
blanca de esquemas o destinos. Si la URL que le pasas a `from_url` puede estar
influenciada por un tercero no confiable (por ejemplo, una "URL de avatar" enviada por
un usuario), descargarla es exactamente igual de peligroso que cualquier otra descarga
del lado del servidor de una entrada no confiable, y restringir qué URLs son seguras de
descargar es responsabilidad de tu aplicación, no de trustmailer. Ver
[Seguridad](/es/docs/security/) para el resto del modelo de amenazas de trustmailer.
:::

## Cabeceras personalizadas de adjunto

`headers: Vec<(String, String)>` añade cabeceras MIME adicionales arbitrarias a la
parte de un único adjunto, añadidas después de las cabeceras estándar
`Content-Type`/`Content-Transfer-Encoding`/`Content-Disposition`/`Content-ID`:

```rust
let attachment = Attachment {
    headers: vec![("X-Attachment-Source".to_string(), "servicio-interno-de-informes".to_string())],
    ..Attachment::from_bytes("informe.pdf", pdf_bytes)
};
```

Toda cabecera que escribe trustmailer — a nivel de adjunto o de mensaje — pasa por la
misma ruta de serialización que elimina CRLF descrita en
[Mensaje → Opciones de cabecera](/es/docs/message/#opciones-de-cabecera), así que un
valor de cabecera aquí controlado por un atacante tampoco puede inyectar una cabecera
falsificada a continuación. A diferencia de `Message::header(name, value)`, sin
embargo, las cabeceras de adjunto no se codifican automáticamente según RFC 2047 para
valores no ASCII — mantén los valores de cabecera de adjunto personalizados en ASCII.

## Desde Node.js

`MailOptions.attachments` es un array de objetos `AttachmentInput`:

```js
const info = await transporter.sendMail({
  from: "alice@example.com",
  to: "bob@example.com",
  subject: "Informe",
  text: "Ver adjunto.",
  html: '<img src="cid:logo123">',
  attachments: [
    { filename: "informe.pdf", content: pdfBuffer, contentType: "application/pdf" },
    { filename: "datos.csv", content: base64Csv }, // un string base64 también funciona
    { filename: "logo.png", content: logoBuffer, contentType: "image/png", cid: "logo123" },
  ],
});
```

| Campo | Tipo | Notas |
|---|---|---|
| `filename` | `string \| undefined` | Mostrado al destinatario. |
| `content` | `Buffer \| string` | Un `Buffer`, o un string codificado en base64 — ambos funcionan, correspondiendo a `Bytes`/`Base64` en el lado de Rust. |
| `contentType` | `string \| undefined` | Tipo MIME explícito; se adivina a partir de `filename` si se omite. |
| `cid` | `string \| undefined` | Fijarlo cambia automáticamente el adjunto a disposición inline — no hace falta fijar `contentDisposition` por separado. |

:::note[Los adjuntos por ruta/URL y las cabeceras por adjunto aún no están en el binding de Node]
`AttachmentInput` solo acepta contenido en memoria (un `Buffer` o un string base64) —
hoy no hay ningún campo `path`/`url` conectado en `trustmailer-node`, ni tampoco un
campo `headers` para cabeceras MIME por adjunto. Ambos existen en el tipo `Attachment`
del core en Rust (ver arriba); si los necesitas desde un proceso Node.js, la opción
actual es descargar o leer el archivo tú mismo en JS y pasar el `Buffer` resultante, o
construir el mensaje directamente con el crate `core`.
:::

Ver [Mensaje](/es/docs/message/) para cómo encajan los adjuntos en la estructura MIME
general, y [Direcciones](/es/docs/addresses/) para cómo se interpretan el resto de
campos del mensaje.
