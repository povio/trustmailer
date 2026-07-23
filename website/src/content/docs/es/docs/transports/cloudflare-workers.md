---
title: Transporte Cloudflare Workers
description: Referencia a fondo de trustmailer-smtp-cfworkers — TCP saliente real de SMTP dentro de un isolate de Worker.
---

`trustmailer-smtp-cfworkers` es un cliente SMTP completo construido específicamente para
ejecutarse **dentro de un Cloudflare Worker**. Habla el protocolo SMTP real sobre la API
`connect()` del módulo `cloudflare:sockets` — TCP saliente genuino, con la plataforma
encargándose del TLS por ti — en lugar de mandar el correo a través de una API HTTP. Si
solo quieres el camino más rápido para enviar un mensaje, mira antes la
[guía rápida de Cloudflare Workers](/es/docs/quickstart/cloudflare-workers/); esta página
es la referencia más profunda para todo lo que esa guía no cubre: cada campo de
configuración, los dos modos de TLS, los mecanismos de autenticación, y exactamente qué
se ha verificado contra una ejecución real de `workerd` frente a lo que solo se ha
comprobado a nivel de configuración.

## Por qué necesita su propio crate

`cloudflare:sockets` es un módulo integrado de `workerd`. Solo se resuelve dentro del
runtime real de Cloudflare Workers — no en una pestaña de navegador, no bajo Deno o Bun
ejecutando el mismo `.wasm` compilado directamente, no bajo `wasm-pack test`. Fuera de un
Worker real, `connect()` simplemente no está definido. Por eso este es un crate separado
de `trustmailer-wasm` en lugar de otra opción `kind` allí: es el único transporte cuyo
entorno objetivo es más estrecho que "cualquier cosa que pueda correr wasm32".

La lógica de protocolo en sí — formato de comandos, análisis de líneas de respuesta,
dot-stuffing, codificación de AUTH — vive en el crate compartido
`trustmailer-smtp-protocol` y es exactamente el mismo código que usa el transporte
nativo `trustmailer-smtp`. Solo difiere la capa de E/S de bytes: este crate maneja el
lector/escritor de Web Streams de `cloudflare:sockets` en lugar de un
`tokio::net::TcpStream`.

## Ejemplo rápido

```js
import wasmModule from "./pkg/trustmailer_smtp_cfworkers_bg.wasm";
import { initSync, createTransport } from "./pkg/trustmailer_smtp_cfworkers.js";

initSync({ module: wasmModule });

export default {
  async fetch() {
    const transporter = createTransport({
      host: "smtp.gmail.com",
      port: 465,
      secure: true,
      auth: {
        type: "login",
        user: "user@gmail.com",
        pass: "app-password",
      },
    });

    try {
      const info = await transporter.sendMail({
        from: "Alice <alice@example.com>",
        to: "bob@example.com",
        replyTo: "support@example.com",
        subject: "Hello from a Worker",
        text: "Sent over a real TCP connection.",
      });
      return Response.json(info);
    } catch (err) {
      // Se lanza como un Error real — err.message, err.stack, `err instanceof Error` funcionan.
      return new Response(err.message, { status: 502 });
    }
  },
};
```

## Referencia de configuración

`createTransport()` recibe un objeto plano con esta forma (los campos desconocidos se
rechazan, no se ignoran en silencio):

| Campo         | Tipo                                   | Valor por defecto | Propósito |
| ------------- | --------------------------------------- | ------------------ | --------- |
| `host`        | `string`                                | — (obligatorio)     | Nombre de host del servidor SMTP. |
| `port`        | `number`                                | — (obligatorio)     | Puerto del servidor SMTP. |
| `secure`      | `boolean`                               | `port === 465`      | `true` para TLS implícito (típicamente puerto 465) — el mismo valor por defecto basado en el puerto que el transporte SMTP nativo, a menos que lo definas explícitamente. Ver [modos de TLS](#modos-de-tls) abajo. |
| `requireTLS`  | `boolean`                               | `true`              | Cuando `secure` es `false`, si la conexión debe actualizarse vía STARTTLS antes de continuar. Desactívalo solo contra un servidor de prueba en texto plano de confianza — ver el aviso abajo. |
| `auth`        | `{ type?, user, pass }`                 | ninguno (sin AUTH)  | Credenciales. `type` es `"plain"` (por defecto si se omite), `"login"`, o `"xoauth2"`; `user`/`pass` se usan como usuario/contraseña para `plain`/`login`, o usuario/token de acceso para `xoauth2`. |
| `name`        | `string`                                | `"localhost"`       | El nombre enviado en el comando `EHLO`. Cualquier CR/LF se elimina antes de usarlo. |

La configuración del lado de Rust (`CfSmtpConfig`, si estás escribiendo tu propio
pegamento `wasm-bindgen` alrededor del crate directamente en vez de pasar por `js.rs`)
expone los mismos controles como métodos builder: `CfSmtpConfig::new(host, port)`,
`.with_secure(bool)`, `.with_auth(Auth::Plain | Auth::Login | Auth::XOAuth2 { .. })`,
`.with_ehlo_name(String)`, `.with_require_tls(bool)`.

## Modos de TLS

| `secure` | `requireTLS` | Qué ocurre |
| :------: | :----------: | ---------- |
| `true`   | *(se ignora)* | TLS implícito: `cloudflare:sockets` abre el socket con `secureTransport: "on"`, así que toda la sesión — incluyendo el saludo inicial — va cifrada desde el primer byte. Corresponde a servidores estilo puerto 465. |
| `false`  | `true` *(por defecto)* | Conecta en claro, envía `EHLO`, y luego exige que el servidor anuncie `STARTTLS` y lo emite, actualizando vía `secureTransport: "starttls"` + el `.startTls()` del socket antes de enviar cualquier credencial o dato de correo. Si el servidor no anuncia `STARTTLS`, la conexión se rechaza con un error en lugar de caer silenciosamente a texto plano. Corresponde a servidores estilo puerto 587. |
| `false`  | `false`      | Texto plano durante toda la sesión, sin intentar actualización. Solo pensado para un servidor local/de prueba de confianza — ver el aviso abajo. |

:::caution[`requireTLS: false` es un control de pruebas, no de producción]
Desactivar `requireTLS` en una conexión no `secure` significa que las credenciales y el
contenido del correo viajan sin cifrar durante toda la sesión. Existe para que las
propias pruebas de integración del crate puedan correr contra un servidor SMTP falso
local sin certificado — no está pensado para activarse contra nada alcanzable en una red
real.
:::

## Autenticación

`auth.type` selecciona qué mecanismo SASL se usa, mapeando al enum compartido
`trustmailer-smtp-protocol::Auth`:

- **`"plain"`** (por defecto si se omite `type`) — un único intercambio `AUTH PLAIN`.
- **`"login"`** — la secuencia clásica de `AUTH LOGIN` con prompts de usuario/contraseña.
- **`"xoauth2"`** — `user` es la cuenta, `pass` se trata como el token de acceso OAuth2.

Antes de intentar el AUTH, el transporte comprueba que el servidor realmente anunció una
capacidad `AUTH` en su respuesta a `EHLO`, y falla con un error claro si no lo hizo, en
lugar de enviar credenciales al vacío y reportar de vuelta un error de protocolo
confuso.

## Esto es SMTP real, no un relay

No hay ninguna pasarela HTTP-a-SMTP en ninguna parte de este camino. El `connect()` de
`cloudflare:sockets` abre un socket TCP saliente genuino desde el isolate del Worker;
este crate habla `EHLO`/`MAIL FROM`/`RCPT TO`/`DATA` reales encima de eso, con la
plataforma manejando el handshake de TLS. La única restricción a nivel de red que impone
Cloudflare es bloquear el puerto 25 saliente (relay MX-a-MX, bloqueado de la misma forma
en cualquier gran proveedor cloud, para frenar el spam) — el envío SMTP por 587/465, que
es lo que este transporte y el "enviar correo por SMTP" cotidiano realmente significan,
no tiene restricción.

Puedes reproducir tú mismo la verificación en vivo:

```sh
cd crates/trustmailer-smtp-cfworkers
wasm-pack build --target web --out-dir test-worker/pkg
cd test-worker
node fake-smtp-server.cjs 2525 &
npx wrangler dev --local
curl "http://127.0.0.1:8787/?host=127.0.0.1&port=2525"
# {"ok":true,"info":{"accepted":["bob@example.com"],"response":"OK: queued as 12345", ...}}
```

:::note[Qué cubre exactamente "verificado en vivo" hoy]
La reproducción de arriba — y la ejecución en vivo de la auditoría contra `workerd` real
— ejercita la **ruta en texto plano** (`secure: false, requireTLS: false`) contra un
servidor SMTP falso local. Las rutas `secure: true` (TLS implícito) y STARTTLS son las
críticas para la seguridad, y hasta la última ronda de auditoría se han verificado
mediante una inspección de código cuidadosa (secuenciación correcta, sin degradación
silenciosa, el búfer de lectura previo al TLS se descarta correctamente durante la
actualización) y con pruebas unitarias a nivel de configuración, pero todavía no se han
ejercitado contra una conexión real de `cloudflare:sockets` que termine TLS de extremo a
extremo. Esa es una afirmación de verificación notablemente más débil que la de la ruta
en texto plano, y se señala aquí explícitamente en lugar de mezclarla en un "verificado
por completo" genérico.
:::

## Corregido: la fuga de socket en la ruta de error

Una versión anterior de este transporte solo ejecutaba `QUIT`/`close()` en la ruta de
éxito. Cualquier error a mitad de una transacción — un destinatario rechazado, un `AUTH`
fallido, un `STARTTLS` que no volvió limpio — regresaba temprano vía `?` antes de que la
limpieza llegara a ejecutarse, filtrando el socket TCP subyacente. `CfSocket` no puede
arreglar esto con un `Drop`, porque la limpieza real es `async` (cerrar el socket implica
esperar una promesa) y el `Drop` de Rust es síncrono. La corrección real es un helper
`close_after` por el que pasan sin condiciones tanto `send()` como `verify()` — `QUIT` y
`close()` ahora se ejecutan en *toda* ruta, éxito o fallo, antes de devolver el resultado
(o error) original a quien llamó. Dejado así en un isolate de Worker de larga duración,
el comportamiento anterior habría acumulado sockets sin cerrar contra el límite de
conexiones por isolate de Cloudflare; esto cierra ese hueco.

## Otro endurecimiento que vale la pena conocer

- **Límite de 8KB en una sola línea de respuesta almacenada en búfer.** `read_line`
  falla rápido con un error claro en lugar de hacer crecer su búfer interno sin límite si
  un servidor (o un proxy mal comportado) nunca envía un terminador de línea —
  protegiendo contra un vector de agotamiento de memoria/caída del isolate dentro del
  presupuesto de memoria limitado de un Worker.
- **Objetos `Error` reales, no strings lanzados a secas.** Los fallos se propagan vía
  `js_sys::Error`, prefiriendo el `.message` propio del valor lanzado cuando ya es un
  `Error`/`DOMException`, así que quien llama obtiene un `.stack` funcional y
  `err instanceof Error` en lugar de un string opaco.
- **Rechazo de campos desconocidos** en `TransportOptions`/`MailInput`/`AuthInput` — un
  nombre de campo JS mal escrito (por ejemplo `scecure`) ahora es un error duro en lugar
  de ignorarse en silencio.

## Limitación conocida y documentada

La fase `DATA` con dot-stuffing actualmente construye el mensaje completo con stuffing
aplicado en memoria antes de escribirlo — para un adjunto grande codificado en base64
eso es 2–3 veces el tamaño del mensaje en memoria a la vez, algo a tener presente dado el
límite por defecto de 128MB de memoria por isolate de un Worker. Transmitirlo en flujo
requeriría una API de dot-stuffing por fragmentos en el crate compartido
`trustmailer-smtp-protocol` (usado también por el transporte SMTP nativo); es un
seguimiento documentado, no algo escondido debajo de la alfombra en silencio.

Comparte una limitación más con el transporte SMTP nativo: enviar a varios destinatarios
es todo o nada — el primer `RCPT TO` que un servidor rechace aborta toda la transacción,
en vez de entregar a los destinatarios que fueron aceptados y reportar el resto como
rechazados como hace nodemailer. Ver la
[referencia del transporte SMTP](/es/docs/transports/smtp/#el-sobre-envelope-smtp) para el
detalle completo; aplica igual aquí.

## Composición con DKIM

`SigningTransport` (de `trustmailer-dkim`) envuelve este transporte de la misma forma
que envuelve cualquier otro, a nivel del código fuente en Rust. Ver
[Firma DKIM](/es/docs/dkim/) para el panorama completo, incluyendo un hueco de
dependencias conocido específico de `wasm32` en el propio `trustmailer-dkim` y el hecho
de que el binding JS de `createTransport()` que se distribuye aquí todavía no expone una
opción `dkim` como sí lo hace el de `trustmailer-node`.

Ver también: la [matriz de runtimes](/es/docs/runtime-matrix/) para comparar este
transporte entre distintos objetivos, [Seguridad](/es/docs/security/) para el panorama
completo de la auditoría, y la
[guía rápida de Cloudflare Workers](/es/docs/quickstart/cloudflare-workers/) para la
configuración mínima.
