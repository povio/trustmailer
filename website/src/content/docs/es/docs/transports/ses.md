---
title: Transporte Amazon SES
description: Entrega de correo por la API HTTPS de SES v2 con un firmante SigV4 hecho a mano — sin dependencia del SDK de AWS.
---

El transporte SES entrega tu mensaje compuesto a través de la API HTTPS `SendEmail` de
la v2 de Amazon Simple Email Service, autenticada con AWS Signature Version 4. Está
implementado en `trustmailer-ses`, reexportado desde el crate fachada como
`trustmailer::ses` tras la feature de Cargo `ses` (activada por defecto).

La decisión distintiva aquí es que trustmailer firma la petición él mismo — no hay
ninguna dependencia de `aws-sdk-sesv2` / `aws-config` en ningún lugar del crate. SigV4
es un algoritmo documentado y estable (HMAC-SHA256 sobre una petición canónica), y
hacerlo a mano significa que este transporte no necesita más que un cliente HTTP para
funcionar — por eso corre sin modificaciones en Rust nativo, Node.js (vía `reqwest`) y
objetivos WASM/navegador (`reqwest` usa `fetch` bajo `wasm32`). Consulta la
[matriz de soporte por runtime](/es/docs/runtime-matrix/).

## Configuración

`SesConfig::new(region, access_key_id, secret_access_key)` es la base obligatoria; todo
lo demás es un método constructor opcional:

| Campo                 | Tipo             | Por defecto                                   | Descripción |
| --------------------- | ---------------- | ----------------------------------------------- | ----------- |
| `region`              | `String`         | *(obligatorio)*                                 | Región de AWS, p. ej. `"us-east-1"`. También determina el host de la API: `email.<region>.amazonaws.com`. |
| `access_key_id`       | `String`         | *(obligatorio)*                                 | ID de clave de acceso de AWS. |
| `secret_access_key`   | `String`         | *(obligatorio)*                                 | Clave de acceso secreta de AWS. |
| `session_token`       | `Option<String>` | `None`                                            | Se define con `.with_session_token(token)` para credenciales temporales de STS; se envía como `x-amz-security-token` y se incorpora a la firma SigV4. |
| `endpoint_override`   | `Option<String>` | `None`                                            | Se define con `.with_endpoint_override(url)`. Sobreescribe el esquema+host al que se hace el POST de la petición — para un endpoint VPC PrivateLink, o para apuntar las pruebas a un servidor simulado. La cabecera `Host` usada para *firmar* no se ve afectada — sigue derivándose de `region`, así que la sobreescritura solo afecta a dónde se envían físicamente los bytes. |
| `message_id_hostname` | `Option<String>` | dominio de envío, con reserva al host de SES      | Se define con `.with_message_id_hostname(hostname)`. Ver más abajo. |

`SesTransport::new(config)` construye un único `reqwest::Client` y lo reutiliza en cada
envío.

### Resolución del hostname del Message-ID

Cuando un `Message` todavía no trae un `message_id`, el transporte SES necesita un
hostname a partir del cual generar uno (la parte después de la `@` en
`<aleatorio@hostname>`). Elige, en este orden:

1. `message_id_hostname`, si lo definiste con `.with_message_id_hostname(...)`.
2. Si no, el dominio de la dirección `From` del mensaje.
3. Si no (no hay dominio `From` disponible), el propio endpoint de la API de SES
   (`email.<region>.amazonaws.com`).

Esa tercera opción es lo que este crate hacía *incondicionalmente* antes, lo que
producía Message-IDs como `<...@email.us-east-1.amazonaws.com>` que no se alinean con
tu dominio de envío real — un problema real de alineación para cualquier verificación
que compare el Message-ID con tu dominio DKIM/SPF. Tanto el valor por defecto basado en
el dominio de `From` como la sobreescritura explícita se añadieron para corregir esto;
prefiere definir `.with_message_id_hostname("tudominio.com")` explícitamente si quieres
un valor garantizado sin importar cuál sea el `From` de un mensaje dado.

## Cómo envía

1. Construye el mensaje a bytes RFC 5322 crudos y los codifica en base64 dentro del
   cuerpo de la petición `SendEmail` de SES v2 como `Content.Raw.Data`, con
   `Destination.ToAddresses` y `FromEmailAddress` tomados del sobre efectivo del
   mensaje.
2. Firma la petición con SigV4 (ver más abajo) y la envía por POST a
   `https://email.<region>.amazonaws.com/v2/email/outbound-emails` (o a tu
   `endpoint_override`).
3. Ante una respuesta que no sea 2xx, devuelve un error que incluye el texto del cuerpo
   de la respuesta (así un "email address not verified" u otro error de SES llega
   intacto a tu código).
4. Si tiene éxito, extrae `MessageId` del cuerpo JSON de la respuesta y lo reporta como
   `SentInfo.message_id` — a diferencia de los transportes sendmail/stream/JSON, este
   es siempre el ID que AWS realmente asignó, no lo que hubiera en la estructura
   `Message` de entrada.

`SentInfo.raw` siempre es `None` en este transporte (los bytes compuestos se enviaron,
no se retuvieron), y `rejected` siempre está vacío — la llamada `SendEmail` de SES v2 es
todo-o-nada en esta integración, así que un fallo parcial se manifiesta como un `Err`,
no como una lista de rechazados.

### El firmante

`trustmailer-ses::sigv4` calcula la petición canónica, la cadena a firmar y la cadena
de claves derivadas (`kDate` → `kRegion` → `kService` → `kSigning`) según la
especificación publicada de SigV4 de AWS, firmando exactamente `host`, `x-amz-date` y
(cuando hay un token de sesión presente) `x-amz-security-token` — el conjunto mínimo de
cabeceras que exige la API de SES. Está cubierto por pruebas que verifican la forma
exacta de la cabecera `Authorization` (`AWS4-HMAC-SHA256
Credential=.../aws4_request, SignedHeaders=..., Signature=...`), además de pruebas de
integración contra un pequeño servidor HTTP/1.1 simulado hecho a mano (sin framework de
mocking, sin llamadas reales a AWS), alcanzado vía `.with_endpoint_override(...)`.

:::tip[Sin SDK de AWS en tu árbol de dependencias]
Como el firmante es autocontenido, añadir soporte de SES a tu binario no arrastra
`aws-sdk-sesv2`, `aws-config` ni su grafo de dependencias transitivas — solo
`trustmailer-ses` y el backend de cliente HTTP que `reqwest` elija para tu objetivo.
:::

## Las credenciales no se imprimen en texto plano

`SesConfig` tiene una implementación de `Debug` escrita a mano específicamente para que
registrar una configuración — `tracing::debug!(?config)`, un mensaje de fallo de
aserción, un pánico — no filtre secretos. `secret_access_key` y (si está definido)
`session_token` siempre se imprimen como `***REDACTED***`; `region`, `access_key_id`,
`endpoint_override` y `message_id_hostname` se imprimen normalmente, ya que no son
sensibles por sí mismos.

:::caution[Verificado, no asumido]
Esto se prueba directamente: construir un `SesConfig` con un secreto y un token de
sesión con aspecto real y formatearlo con `{:?}` verifica que los valores secretos
nunca aparecen en la salida y que `***REDACTED***` sí lo hace. Consulta
[Seguridad](/es/docs/security/) para el patrón más amplio en el que esto encaja.
:::

## `verify()`

`SesTransport` no sobreescribe `Transport::verify()`, así que hereda el valor por
defecto — `Ok(true)` incondicionalmente, sin ninguna llamada de red real. Si quieres
confirmar que tus credenciales de SES realmente funcionan, envía un mensaje real (a una
dirección verificada, si tu cuenta sigue en el sandbox de SES) en lugar de confiar en
`.verify()`.

## Ejemplo en Rust

```rust
use trustmailer::{Message, Transport};
use trustmailer::ses::{SesConfig, SesTransport};

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let config = SesConfig::new(
        "us-east-1",
        std::env::var("AWS_ACCESS_KEY_ID").unwrap(),
        std::env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
    )
    .with_message_id_hostname("example.com");

    let transport = SesTransport::new(config);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello from SES")
        .text("Hi Bob!")
        .build();

    let info = transport.send(&message).await?;
    println!("SES message id: {}", info.message_id);
    Ok(())
}
```

## Node.js, Bun y Deno

La opción `ses` de `createTransport()` construye un `SesTransport` directamente — sin
SMTP de por medio, la misma ruta HTTPS + SigV4 hecho a mano que en Rust nativo:

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  ses: {
    region: "us-east-1",
    accessKeyId: process.env.AWS_ACCESS_KEY_ID,
    secretAccessKey: process.env.AWS_SECRET_ACCESS_KEY,
    // sessionToken: "...",             // opcional, para credenciales temporales/STS
    // endpointOverride: "https://...", // opcional, VPC PrivateLink o un servidor simulado
    // messageIdHostname: "example.com", // opcional, ver "Hostname del Message-ID" arriba
  },
});

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello from SES",
  text: "Hi Bob!",
});
console.log("SES message id:", info.messageId);
```

Los campos de `SesOptions` reflejan uno a uno el builder de `SesConfig` (`region`/
`accessKeyId`/`secretAccessKey` requeridos, el resto opcional) — verificado contra el
`index.d.ts` generado de `crates/trustmailer-node/src/lib.rs`, no asumido a partir de la
API de Rust. `dkim` se compone con él exactamente igual que con cualquier otra opción de
transporte (`createTransport({ ses: {...}, dkim: {...} })`).

## Disponibilidad en navegador/WASM

`trustmailer-wasm` (navegador, Cloudflare Workers, otros hosts WASM) también expone SES,
con una superficie de opciones más pequeña que el crate de Rust y que el binding de
Node — solo `{ kind: "ses", region, accessKeyId, secretAccessKey }`, sin `sessionToken`,
`endpointOverride` ni `messageIdHostname` conectados por ahora. Consulta la
[guía rápida de navegador/WASM](/es/docs/quickstart/browser-wasm/).

## Ver también

- [Transporte Sendmail](/es/docs/transports/sendmail/) — entrega vía binario local en
  lugar de una API HTTPS.
- [Transporte Stream](/es/docs/transports/stream/) y
  [Transporte JSON](/es/docs/transports/json/) — renderizan sin enviar, útiles para
  probar mensajes destinados a SES sin llamadas reales a AWS.
- [Seguridad](/es/docs/security/) — la auditoría más amplia de la que forman parte el
  `Debug` redactado y las pruebas de SigV4 de este transporte.
- [Matriz de soporte por runtime](/es/docs/runtime-matrix/) — disponibilidad completa
  por runtime.
