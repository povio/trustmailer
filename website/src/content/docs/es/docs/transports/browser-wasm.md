---
title: Transporte navegador / WASM
description: Referencia a fondo de trustmailer-wasm — qué soporta en una pestaña de navegador, y por qué SMTP genuinamente no puede ser una de esas cosas.
---

`trustmailer-wasm` es el binding `wasm-bindgen` para hosts que son un sandbox genuino sin
ninguna vía de escape a código nativo ni a TCP crudo: una pestaña de navegador, ante
todo, pero el mismo módulo compilado también corre bajo Deno, Bun, y (si no necesitas
SMTP saliente real) dentro de un Cloudflare Worker. Para el camino más rápido a enviar un
mensaje, mira antes la
[guía rápida de navegador/WASM](/es/docs/quickstart/browser-wasm/); esta página cubre
cada campo de configuración, ambos backends soportados en detalle, y la razón
arquitectónica por la que SMTP no es — ni puede ser — una tercera opción aquí.

## Qué está soportado, y qué deliberadamente no lo está

`createTransport()` acepta exactamente uno de tres `kind`, confirmados directamente
desde el enum `TransportOptions` del crate:

- **`"stream"`** — renderiza el mensaje a bytes MIME crudos y los devuelve, sin enviar
  nada. Tú te encargas del resto (una llamada `fetch` a tu propio backend, tu propio
  relay, donde sea que el correo realmente necesite ir).
- **`"json"`** — la misma idea, pero devuelve una representación JSON del mensaje en
  lugar de MIME crudo.
- **`"ses"`** — envía vía la API HTTPS de Amazon SES. Este es el único backend aquí que
  realmente entrega correo por sí mismo, porque es una simple petición HTTPS — el mismo
  mecanismo que usa `fetch`, por el que `reqwest` (el cliente HTTP sobre el que está
  construido este crate) enruta de forma transparente bajo `wasm32`.

Deliberadamente **no existe un `kind` `"smtp"`**. Esto no es un descuido ni una
funcionalidad faltante para abrir un ticket:

:::caution[Sin TCP crudo en una pestaña de navegador — para cualquiera, no solo para trustmailer]
Una pestaña de navegador real no tiene ninguna API de TCP crudo ni de código nativo.
`cloudflare:sockets` (usado por el [transporte de Cloudflare Workers](/es/docs/transports/cloudflare-workers/)
separado) es un integrado específico de `workerd` que simplemente no existe fuera de un
isolate de Worker real — no es en absoluto una API de navegador. Ninguna librería de
correo en JavaScript, trustmailer incluido, puede abrir un socket SMTP desde una página
cargada en una URL en una pestaña normal; el mismo sandbox que hace segura de visitar una
pestaña de navegador es exactamente lo que descarta esto. Ver la
[matriz de runtimes](/es/docs/runtime-matrix/) para el desglose completo de qué es
arquitectónico frente a qué es un hueco real en otra parte del proyecto.
:::

## Ejemplos rápidos

### SES (realmente envía correo)

```js
import init, { createTransport } from "trustmailer-wasm";

await init();

const transporter = createTransport({
  kind: "ses",
  region: "us-east-1",
  accessKeyId: "AKIA...",
  secretAccessKey: "...",
});

try {
  const info = await transporter.sendMail({
    from: "alice@example.com",
    to: "bob@example.com",
    replyTo: "support@example.com",
    subject: "Hello",
    text: "Hi Bob!",
    html: "<p>Hi Bob!</p>",
  });
  console.log(info.messageId, info.accepted, info.rejected);
} catch (err) {
  // Un Error real — err.message, err.stack, `err instanceof Error` funcionan.
  console.error(err.message);
}
```

### Stream (renderiza, y entrega tú mismo)

```js
import init, { createTransport } from "trustmailer-wasm";

await init();

const transporter = createTransport({ kind: "stream" });

const info = await transporter.sendMail({
  from: "alice@example.com",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});

// `info.response` contiene el MIME crudo renderizado — pásalo a tu propio backend/relay.
await fetch("/api/relay-outbound-mail", {
  method: "POST",
  headers: { "Content-Type": "text/plain" },
  body: info.response,
});
```

Cambia `kind: "stream"` por `kind: "json"` para recibir una representación JSON en
`response` en lugar de texto MIME crudo — la misma idea, distinta serialización según lo
que espere tu relay.

## Referencia de configuración

El argumento de `createTransport()` es un objeto etiquetado; los campos desconocidos se
rechazan en lugar de ignorarse en silencio:

| `kind`     | Campo               | Obligatorio | Propósito |
| ---------- | -------------------- | :---------: | --------- |
| cualquiera | `kind`                | sí          | `"stream"`, `"json"`, o `"ses"`. |
| `"ses"`    | `region`              | sí          | Región de AWS, p. ej. `"us-east-1"`. |
| `"ses"`    | `accessKeyId`         | sí          | ID de clave de acceso de AWS. |
| `"ses"`    | `secretAccessKey`     | sí          | Clave de acceso secreta de AWS. |

El objeto de mensaje de `sendMail()` refleja los mismos campos en los tres backends:

| Campo       | Tipo     | Notas |
| ----------- | -------- | ----- |
| `from`      | `string` | |
| `to`        | `string` | |
| `cc`        | `string` | |
| `bcc`       | `string` | |
| `replyTo`   | `string` | |
| `subject`   | `string` | |
| `text`      | `string` | |
| `html`      | `string` | |

Actualmente no hay un campo `attachments` en el `MailInput` de este binding — los
adjuntos aún no son alcanzables a través de esta superficie JS de navegador/WASM aunque
`trustmailer-core::Message` los soporta por completo.

La respuesta de `sendMail()` es un objeto plano (no un `Map` de JS — ver el aviso abajo)
con los campos `messageId`, `accepted`, `rejected`, y `response`.

:::caution[Credenciales de SES en una pestaña de navegador]
Enviar una clave de acceso secreta de AWS de larga duración a código que corre en el
navegador de alguien significa que es visible para esa persona (y para cualquiera que
abra las herramientas de desarrollador). El `kind` `"ses"` es una integración HTTPS
directa y correcta — pero para un producto real de cara al navegador, enrútalo a través
de un backend que guarde las credenciales reales (o que emita unas de corta duración y
alcance limitado) en lugar de incrustar una clave secreta de larga duración en
JavaScript del lado del cliente. Los `kind` `"stream"`/`"json"` junto con tu propio
endpoint de backend autenticado son la forma por defecto más segura para cualquier cosa
que realmente dé la cara al usuario final.
:::

## Corregido: el mismo error de respuesta `{}`

Una versión anterior tanto de este crate como de `trustmailer-smtp-cfworkers` devolvía
`{}` en cada llamada a `sendMail()` — no un error, solo datos silenciosamente
incorrectos. La causa: `serde-wasm-bindgen` serializa los campos de struct de Rust como
un `Map` de JS por defecto, y un `Map` no tiene propiedades enumerables propias, así que
`JSON.stringify()`/`Response.json()` lo renderizaban como un objeto vacío sin que se
lanzara ningún error en ninguna parte. Esto se encontró ejecutando de verdad la ruta de
Cloudflare Workers en vivo de punta a punta e inspeccionando la respuesta real — no
mediante una revisión solo de documentación o solo de compilación, que jamás lo habría
sacado a la luz. La corrección, `serialize_maps_as_objects(true)`, se aplica en cada
punto de llamada a `sendMail()` de este crate.

## Otro endurecimiento que vale la pena conocer

- **Objetos `Error` reales, no strings lanzados a secas** — los fallos se lanzan vía
  `js_sys::Error`, así que quien llama obtiene un `.stack` funcional y
  `err instanceof Error` en lugar de un string opaco.
- **Rechazo de campos desconocidos** tanto en `TransportOptions` como en `MailInput` —
  un nombre de campo mal escrito es un error duro en lugar de descartarse en silencio.

## Composición con DKIM

A nivel del código fuente en Rust, `SigningTransport` (de `trustmailer-dkim`) es genérico
sobre cualquier `Transport` y tiene una ruta de firma en línea dedicada para
`target_arch = "wasm32"` (en lugar de `spawn_blocking`, ya que `wasm32` no tiene pool de
hilos bloqueantes y es de un solo hilo de todos modos) — así que el soporte para
`wasm32` claramente se tuvo en mente. En la práctica, compilar `trustmailer-dkim` para
`wasm32-unknown-unknown` tropieza hoy con un hueco conocido y preexistente en su árbol de
dependencias (la dependencia transitiva `rand` → `getrandom` del crate `rsa` necesita una
funcionalidad de entropía para WASM que nada en el grafo habilita actualmente);
`trustmailer-wasm` en sí no depende en absoluto de `trustmailer-dkim`/`rsa`, así que ese
hueco no le afecta. Por separado, el binding JS `createTransport()` que distribuye este
crate no expone actualmente una opción `dkim` como sí lo hace el de `trustmailer-node`,
así que la firma DKIM tampoco es alcanzable desde esta superficie JS de fábrica hoy por
esa otra razón. Ver [Firma DKIM](/es/docs/dkim/) para el panorama completo.

Ver también: la [matriz de runtimes](/es/docs/runtime-matrix/) para el desglose completo
por transporte y por runtime, [Seguridad](/es/docs/security/) para el panorama completo
de la auditoría, y la
[guía rápida de navegador/WASM](/es/docs/quickstart/browser-wasm/) para la configuración
mínima.
