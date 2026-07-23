---
title: Transporte SMTP
description: Referencia completa del transporte SMTP nativo de trustmailer — conexión, TLS/STARTTLS, autenticación, OAuth2, pooling, proxy SOCKS5, timeouts y servicios conocidos.
---

El transporte SMTP (`trustmailer-smtp`) es el transporte principal de trustmailer: un
cliente SMTP escrito desde cero sobre `tokio`, que implementa él mismo la secuencia
conectar → `EHLO` → STARTTLS opcional → `AUTH` opcional → transacción
`MAIL FROM`/`RCPT TO`/`DATA`, en lugar de delegar en un MTA del sistema o en otra
librería. Es exclusivo de entornos nativos (TCP crudo + `tokio-rustls`), por lo que está
disponible desde Rust nativo y desde el addon nativo de Node.js/Bun/Deno; los entornos de
navegador/Workers usan [`trustmailer-smtp-cfworkers`](/es/docs/quickstart/cloudflare-workers/)
o `trustmailer-wasm` en su lugar — consulta la
[matriz de runtimes](/es/docs/runtime-matrix/) para el panorama completo.

Todo lo que sigue está verificado contra el struct `SmtpConfig` real y sus métodos
constructores `with_*` en `crates/trustmailer-smtp/src/config.rs` — si una opción no
aparece aquí, trustmailer no la tiene (todavía).

## Crear un transporte

```rust
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};
use trustmailer::Transport;

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() });
let transport = SmtpTransport::new(config);
```

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  host: "smtp.example.com",
  port: 587,
  auth: { user: "user", pass: "pass" },
});
```

`SmtpConfig::new(host, port)` es el punto de entrada en Rust; cada ajuste adicional es una
llamada `with_*` encadenada sobre él (los campos del struct son `pub`, pero los
constructores además normalizan la entrada — por ejemplo, `with_ehlo_name` elimina los
CRLF — así que conviene usarlos en vez de asignar los campos directamente). El binding de
Node.js construye un `SmtpConfig` a partir de un objeto plano de JS; consulta
[soporte en Node.js](#soporte-en-nodejs-bun-deno) más abajo para ver exactamente qué
campos reenvía.

## El sobre (envelope) SMTP

`MAIL FROM`/`RCPT TO` — las direcciones a las que el propio protocolo SMTP realmente
entrega — se calculan a partir del mensaje, no se leen de una config separada aquí. Ver
[Mensaje → Envelope y `effective_envelope()`](/es/docs/message/#envelope-y-effective_envelope)
para cómo `to`/`cc`/`bcc`/`from` derivan el sobre por defecto, cuándo sobreescribirlo con
`.envelope(...)` (manejo de rebotes de listas de correo, destinatarios `Bcc` — entregables
pero nunca presentes en ningún header), y los errores `NoRecipients`/`NoSender` que se
lanzan cuando ni los headers ni una sobreescritura explícita dejan algo a quién enviar.

:::caution[Sin entrega parcial a destinatarios, a diferencia de nodemailer]
Con varios destinatarios, nodemailer entrega a las direcciones `RCPT TO` que el servidor
acepte y reporta el resto vía `info.rejected`, incluso si algunas son rechazadas — solo el
rechazo total falla el envío. `SmtpTransport::send` no hace esto hoy: `send_transaction`
(`crates/trustmailer-smtp/src/connection.rs`) aborta *toda* la transacción ante la
**primera** respuesta `RCPT TO` que no sea `250` — no se envía ningún `DATA`, así que una
sola dirección mala en una lista grande de `to`/`cc` bloquea la entrega a todos los demás
destinatarios también. Por consiguiente, `SentInfo.rejected` siempre está vacío en este
transporte (`accepted` siempre refleja el sobre completo) — no hay información de fallo
parcial que reportar, porque el fallo parcial no puede ocurrir; es todo o nada. Esto
también aplica al transporte SMTP de Cloudflare Workers (`trustmailer-smtp-cfworkers`),
que comparte la misma forma de bucle. Si tu caso de uso de lista de correo necesita
resultados por destinatario, envía un mensaje por destinatario hasta que esto se resuelva.
:::

## Conexión básica: host, port, secure, require_tls

| Campo | Tipo | Valor por defecto | Descripción |
|---|---|---|---|
| `host` | `String` | — (obligatorio) | Hostname o IP del servidor SMTP. |
| `port` | `u16` | — (obligatorio) | Puerto del servidor SMTP. |
| `secure` | `bool` | `port == 465` | Conecta con **TLS implícito**: el handshake TLS ocurre inmediatamente después del connect TCP, antes de enviar ningún comando SMTP. |
| `require_tls` | `bool` | `port == 587` | Exige un upgrade **STARTTLS** durante la fase en texto plano; la conexión falla directamente si el servidor no lo anuncia. |

`SmtpConfig::new` deriva valores por defecto sensatos solo a partir del número de puerto:
el puerto `465` activa `secure: true` (TLS implícito, la convención SMTPS tradicional), el
puerto `587` activa `require_tls: true` (la convención moderna del puerto de envío), y
cualquier otro puerto deja ambos en `false`. Llamar a `.with_secure(true)` también pone
`require_tls` en `false` (y viceversa), ya que ambos son mutuamente excluyentes — `secure`
cifra desde el primer byte, `require_tls` empieza en texto plano y hace el upgrade a mitad
de la conversación.

### TLS implícito vs STARTTLS vs upgrade oportunista

Son tres momentos distintos del ciclo de vida de la conexión en los que puede empezar el
cifrado, y el código de conexión de trustmailer
(`crates/trustmailer-smtp/src/connection.rs`) los maneja explícitamente:

1. **`secure: true`** — TLS envuelve el socket antes de enviar siquiera `EHLO`. Es lo que
   siempre ha significado el puerto 465.
2. **`require_tls: true`** — la conexión empieza en texto plano, envía `EHLO`, y si el
   servidor no incluye `STARTTLS` en su respuesta de capacidades, el connect falla de
   inmediato con `"server does not advertise STARTTLS"` en vez de caer silenciosamente a
   texto plano.
3. **Upgrade oportunista (el comportamiento por defecto cuando ninguna de las anteriores
   fuerza el resultado)** — incluso sin `require_tls`, si la respuesta `EHLO` del servidor
   anuncia `STARTTLS`, trustmailer lo negocia de todos modos. El razonamiento detrás de
   esto: no hay ninguna buena razón para dejar una conexión en texto plano solo porque
   quien la configuró no pidió TLS explícitamente, cuando el servidor lo acaba de ofrecer
   gratis.

:::tip[Auditado de forma independiente]
El comportamiento de upgrade oportunista de arriba es deliberado, no accidental — una
versión anterior hacía el upgrade *incondicional* (sin forma de rechazarlo), lo que rompía
relays internos con certificado autofirmado que antes funcionaban bien en texto plano. Una
segunda auditoría independiente detectó la regresión, y la vía de escape
`disable_starttls` de abajo se añadió específicamente para solucionarlo. Consulta
[Seguridad](/es/docs/security/) para el relato completo de qué se encontró y qué se
corrigió.
:::

Usa `with_disable_starttls(true)` para rechazar el upgrade oportunista incluso cuando el
servidor ofrece `STARTTLS` — útil para un relay interno cuyo certificado autofirmado/no
confiable, de otro modo, fallaría contra el almacén de confianza `webpki-roots` fijo de
trustmailer (todavía no hay opción de CA personalizada, así que esta es hoy la única forma
de seguir hablando con ese relay). Este flag se **ignora** cuando `require_tls` está
activado, ya que ese ajuste exige TLS explícitamente y un fallo directo es el resultado más
seguro en ese caso.

```rust
// Relay interno, certificado autofirmado, en el 587 pero sin forzar STARTTLS:
let config = SmtpConfig::new("relay.internal", 587)
    .with_disable_starttls(true);
```

## Autenticación

`Auth` (de `trustmailer-smtp-protocol`) tiene tres variantes, cada una asociada a un
mecanismo SASL estándar:

| Variante | Mecanismo SASL | Campos |
|---|---|---|
| `Auth::Plain` | `PLAIN` | `username`, `password` |
| `Auth::Login` | `LOGIN` | `username`, `password` |
| `Auth::XOAuth2` | `XOAUTH2` | `username`, `access_token` |

```rust
let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() });
```

Antes de enviar `AUTH`, la conexión verifica que la respuesta `EHLO` del servidor
realmente anunció ese mecanismo concreto (por ejemplo `AUTH PLAIN LOGIN`), y falla rápido
con `"server does not advertise AUTH <mechanism>"` si no es así — en vez de enviar
credenciales al vacío y recibir un rechazo genérico. `Auth::Plain`/`Auth::XOAuth2` envían
sus credenciales en un único comando `AUTH <mechanism> <base64>` (respuesta inicial);
`Auth::Login` hace el intercambio de reto/respuesta de RFC 4954 (`AUTH LOGIN` → el
servidor pregunta → usuario en base64 → el servidor pregunta → contraseña en base64).

### La protección `allow_insecure_auth`

| Campo | Tipo | Valor por defecto | Descripción |
|---|---|---|---|
| `allow_insecure_auth` | `bool` | `false` | Permite enviar `AUTH` sobre una conexión que no es `secure` ni fue actualizada vía STARTTLS. |

:::caution[Una protección de seguridad, no solo un flag]
Por defecto, trustmailer **se niega a enviar `AUTH` en texto plano**. Si hay
autenticación configurada pero la conexión no terminó siendo `secure: true` ni
actualizada por STARTTLS (por ejemplo, un relay interno mal configurado en un puerto no
estándar que no ofrece `STARTTLS`), `SmtpConnection::connect` devuelve un error —
`"refusing to send AUTH over a non-TLS connection (set SmtpConfig::allow_insecure_auth to
override)"` — en vez de poner silenciosamente una contraseña en el cable en texto plano.
Este es un caso de mala configuración real, no hipotético: es exactamente el tipo de error
que filtra credenciales a quien pueda observar la conexión. Activa
`with_allow_insecure_auth(true)` solo cuando hayas decidido deliberadamente que es
aceptable (por ejemplo, una conexión loopback a un servidor de pruebas local).
:::

```rust
// Mail-catcher local en el puerto 1025 en texto plano — no hay TLS disponible, y aquí está bien.
let config = SmtpConfig::new("localhost", 1025)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() })
    .with_allow_insecure_auth(true);
```

### OAuth2 (XOAUTH2)

Para proveedores como Gmail que admiten OAuth2 en lugar de una contraseña estática,
`OAuth2Config` + `OAuth2TokenProvider` (en `trustmailer-smtp::oauth2`, solo Rust nativo —
ver la sección de Node.js más abajo) gestionan el flujo de renovación de tokens: registras
una app OAuth, obtienes un `refresh_token` una vez mediante el flujo de consentimiento del
proveedor, y luego dejas que el proveedor lo cambie por access tokens de corta duración
según se necesiten.

```rust
use trustmailer::smtp::{OAuth2Config, OAuth2TokenProvider, SmtpConfig, SmtpTransport};

let oauth2 = OAuth2Config::new(
    "user@gmail.com",
    "client-id.apps.googleusercontent.com",
    "client-secret",
    "refresh-token",
);
let provider = OAuth2TokenProvider::new(oauth2);

let config = SmtpConfig::new("smtp.gmail.com", 465)
    .with_secure(true)
    .with_auth(provider.to_auth().await?);
let transport = SmtpTransport::new(config);
```

| Campo | Tipo | Valor por defecto | Descripción |
|---|---|---|---|
| `user` | `String` | — (obligatorio) | Casilla a autenticar. |
| `client_id` | `String` | — (obligatorio) | ID de cliente OAuth2. |
| `client_secret` | `String` | — (obligatorio) | Secreto de cliente OAuth2. |
| `refresh_token` | `String` | — (obligatorio) | Refresh token de larga duración del flujo de consentimiento. |
| `access_url` | `String` | `https://accounts.google.com/o/oauth2/token` | Endpoint de tokens; sobrescríbelo con `.with_access_url(...)` para un proveedor distinto de Google. |

`OAuth2TokenProvider::get_access_token()` (usado internamente por `to_auth()`) guarda en
caché el access token obtenido hasta poco antes de que caduque según su `expires_in` (un
margen de seguridad de 60 segundos absorbe el desfase de reloj y la latencia en curso), de
forma que llamadas repetidas — o varias conexiones en pool compartiendo un mismo
proveedor — no vuelvan a golpear el endpoint de tokens cada vez.

:::caution[Solo HTTPS, con una excepción muy acotada]
`access_url` se rechaza directamente a menos que sea `https://`, **excepto** para una
autoridad loopback exacta (`127.0.0.1`, `localhost` o `[::1]`, opcionalmente con
puerto/ruta) — esa excepción existe únicamente para que los tests propios de la librería
puedan apuntar a un endpoint de tokens simulado en local. La comparación es de autoridad
exacta, no `starts_with`: `http://127.0.0.1.evil.com` o `http://localhost@evil.com` se
rechazan correctamente, no se tratan como loopback. Una versión anterior, menos cuidadosa,
de esta comprobación usaba un `starts_with` a secas y fue detectada y corregida durante una
segunda auditoría de seguridad independiente — consulta [Seguridad](/es/docs/security/)
para más detalles. Enviar `client_secret`/`refresh_token` a una URL en texto plano
alcanzable por un atacante filtraría ambos de forma permanente.
:::

## Servicios conocidos

`SmtpConfig::for_service(name)` busca host/puerto/secure para un proveedor conocido por
nombre, alias, dominio simple o dirección de correo completa — sin distinguir mayúsculas
ni puntuación — vía `trustmailer_core::well_known::lookup_service`. Se reconocen unos 85
proveedores, portados desde el propio `well-known/services.json` de nodemailer; algunos
ejemplos directamente de la tabla:

| Clave de búsqueda | Resuelve a |
|---|---|
| `"gmail"` / `"Google Mail"` / `"gmail.com"` / `"user@gmail.com"` | `smtp.gmail.com:465`, `secure: true` |
| `"outlook"` / `"hotmail"` | `smtp-mail.outlook.com:587`, `secure: false` (STARTTLS) |
| `"ses"` (por defecto `us-east-1`; también existen claves por región como `"sesEuWest1"`) | `email-smtp.us-east-1.amazonaws.com:465`, `secure: true` |

```rust
let config = SmtpConfig::for_service("gmail")
    .expect("servicio desconocido")
    .with_auth(Auth::Plain { username: "user@gmail.com".into(), password: "app-password".into() });
```

```js
const transporter = createTransport({
  service: "gmail",
  auth: { user: "user@gmail.com", pass: "app-password" },
});
```

`for_service` devuelve `None` para un nombre no reconocido — nunca adivina un host, así
que conviene comprobar el resultado (o manejar deliberadamente el pánico de
`unwrap`/`expect`) en lugar de asumir que cualquier cadena se resuelve.

## Pooling de conexiones

`SmtpTransport::pooled()` (o `SmtpPool::new(config)` directamente) envuelve una
configuración en un pool que reutiliza conexiones ya autenticadas entre envíos, en lugar
de conectar, hacer el handshake TLS y autenticarse de nuevo para cada mensaje — reflejando
la opción `pool: true` de nodemailer.

```rust
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};
use trustmailer::Transport;

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() })
    .with_pool_size(10);
let pool = SmtpTransport::new(config).pooled();

for message in messages {
    pool.send(&message).await?;
}
pool.close().await;
```

| Campo | Tipo | Valor por defecto | Descripción |
|---|---|---|---|
| `pool_size` | `usize` | `5` | Máximo de conexiones concurrentes que el pool mantiene abiertas (el checkout se bloquea al superar este límite, mediante un permiso de `tokio::sync::Semaphore`). |

Al hacer checkout, una conexión del pool no se entrega a ciegas: el pool envía primero un
`NOOP` para confirmar que sigue viva, ya que los servidores SMTP habitualmente cierran
conexiones inactivas tras su propio timeout. Una conexión muerta se descarta y el pool
prueba con la siguiente conexión inactiva o marca una nueva — de modo que una conexión
obsoleta se traduce en una reconexión transparente, no en un error de I/O crudo en tu
primer comando real. En el checkin (tras un envío exitoso), la conexión vuelve a la lista
de inactivas solo si está por debajo de `pool_size`; si no, se descarta. Ante un envío
fallido, la conexión se cierra (`QUIT`) en vez de devolverse al pool, ya que su estado tras
un error es desconocido. `pool.close()` envía `QUIT` a cada conexión actualmente inactiva;
los envíos en curso no se ven afectados.

:::note[Node.js/Bun/Deno]
El pooling de conexiones es, por ahora, exclusivo de Rust nativo — `TransportOptions` en
`trustmailer-node` no expone un campo `pool`/`pool_size`, así que `createTransport()`
siempre devuelve un transporte de una sola conexión. Consulta
[soporte en Node.js](#soporte-en-nodejs-bun-deno).
:::

## Proxy SOCKS5

`ProxyConfig` túnela la conexión SMTP a través de un proxy SOCKS5 (reflejando la opción
`proxy: "socks5://..."` de nodemailer). Hoy solo se soporta SOCKS5 — los proxies HTTP
CONNECT no están implementados.

```rust
use trustmailer::smtp::{Auth, ProxyConfig, SmtpConfig, SmtpTransport};

let proxy = ProxyConfig::new("proxy.internal", 1080)
    .with_auth("proxy-user", "proxy-pass"); // omite .with_auth(...) para un proxy sin autenticación

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_auth(Auth::Plain { username: "user".into(), password: "pass".into() })
    .with_proxy(proxy);
let transport = SmtpTransport::new(config);
```

`ProxyConfig::parse("socks5://user:pass@proxy.internal:1080")` parsea directamente una
URL de proxy (aceptando también el esquema `socks5h://` como alias) si prefieres no
construirla campo a campo.

:::caution[Sin degradación silenciosa a autenticación anónima]
Los campos `username`/`password` de `ProxyConfig` son públicos, así que es posible
construir uno con solo *uno* de los dos definido (evitando `with_auth`). trustmailer trata
eso como un error de configuración duro en el momento de conectar, en lugar de caer
silenciosamente a autenticación SOCKS5 anónima — quien definió un usuario claramente
pretendía un proxy autenticado, y degradar eso en silencio sería una regresión de
seguridad que nadie pidió.
:::

:::note[Node.js/Bun/Deno]
Igual que el pooling, el proxy SOCKS5 es hoy exclusivo de Rust nativo — no está expuesto a
través de `TransportOptions` de `trustmailer-node`.
:::

## Timeouts

| Campo | Tipo | Valor por defecto | Descripción |
|---|---|---|---|
| `connect_timeout` | `Duration` | `30s` | Limita el connect TCP inicial **y** el handshake TLS/STARTTLS. |
| `command_timeout` | `Duration` | `60s` | Limita el ciclo escritura+lectura de cualquier comando SMTP individual, incluida la lectura completa de una respuesta multilínea. |

Cada operación de red dentro de la conexión — connect, handshake TLS, cada comando, la
lectura de cada respuesta (potencialmente multilínea) y la escritura final de `DATA` —
pasa por un mismo envoltorio de timeout compartido, de modo que un servidor que se
atasca en cualquier paso concreto produce un error de transporte claro
(`"operation timed out after ..."`) en vez de dejar colgado al llamador para siempre. El
parseo de respuestas también está acotado de forma independiente al timeout: una sola
línea de respuesta tiene un tope de 64 KiB, y una respuesta multilínea tiene un tope de
1000 líneas / 256 KiB en total, así que un servidor que se comporte mal no puede hacer
crecer la memoria del cliente sin límite ni siquiera dentro de la ventana del timeout.

```rust
use std::time::Duration;

let config = SmtpConfig::new("smtp.example.com", 587)
    .with_connect_timeout(Duration::from_secs(10))
    .with_command_timeout(Duration::from_secs(20));
```

## El atajo de cadena de conexión `from_url`

`SmtpConfig::from_url(url)` parsea `smtp://[user[:pass]@]host[:port]` (texto plano/STARTTLS)
o `smtps://...` (TLS implícito) en una sola llamada — reflejando el atajo de nodemailer
`nodemailer.createTransport("smtps://user:pass@smtp.gmail.com")`.

```rust
let config = SmtpConfig::from_url("smtps://user%40gmail.com:app-password@smtp.gmail.com")?;
let transport = SmtpTransport::new(config);
```

Notas sobre el comportamiento del parseo, todas verificadas por los propios tests de la
librería:

- El puerto tiene un valor por defecto según el esquema cuando se omite: `465` para
  `smtps://`, `587` para `smtp://`.
- La información de usuario (`user:pass@`) se decodifica de porcentaje (`%40` → `@`,
  etc.), así que una dirección de correo como usuario funciona directamente en la URL.
- Se admite una credencial solo-contraseña (`smtps://:api-key@host`) — el usuario queda
  simplemente vacío — para proveedores que autentican con una API key a secas.
- Solo se usa la autoridad (`user:pass@host:port`); cualquier ruta, query string o
  fragmento en la URL se analiza y se descarta, no se rechaza.
- Un esquema no soportado (cualquiera que no sea `smtp`/`smtps`) o un host ausente es un
  error de parseo duro, no un intento de fallback.

`from_url` es hoy exclusivo de Rust nativo; todavía no está conectado a
`TransportOptions` de `trustmailer-node` (que en su lugar toma `host`/`port`/`secure` como
campos separados).

## Verificar la configuración

Todos los transportes (SMTP incluido) implementan `verify()`, reflejando el
`transporter.verify()` de nodemailer: para SMTP esto abre una conexión real (pasando por
toda la secuencia connect/TLS/STARTTLS/AUTH descrita arriba), envía un `NOOP` y la cierra
— una forma de confirmar que host/puerto/credenciales son correctos antes de depender de
ello en medio de un envío real.

```rust
transport.verify().await?; // Ok(true) si tiene éxito, Err(...) con el motivo si no
```

```js
const ok = await transporter.verify();
```

## Referencia completa de `SmtpConfig`

| Campo | Tipo | Valor por defecto | Se define con |
|---|---|---|---|
| `host` | `String` | — (obligatorio) | `SmtpConfig::new(host, port)` |
| `port` | `u16` | — (obligatorio) | `SmtpConfig::new(host, port)` |
| `secure` | `bool` | `port == 465` | `.with_secure(bool)` (también invierte `require_tls`) |
| `require_tls` | `bool` | `port == 587` | derivado de `.with_secure(bool)`, o definido directamente |
| `auth` | `Option<Auth>` | `None` | `.with_auth(Auth::Plain \| Login \| XOAuth2 { .. })` |
| `ehlo_name` | `String` | `"localhost"` | `.with_ehlo_name(name)` (se eliminan los CRLF) |
| `pool_size` | `usize` | `5` | `.with_pool_size(n)` |
| `proxy` | `Option<ProxyConfig>` | `None` | `.with_proxy(ProxyConfig)` |
| `connect_timeout` | `Duration` | `30s` | `.with_connect_timeout(Duration)` |
| `command_timeout` | `Duration` | `60s` | `.with_command_timeout(Duration)` |
| `allow_insecure_auth` | `bool` | `false` | `.with_allow_insecure_auth(bool)` |
| `disable_starttls` | `bool` | `false` | `.with_disable_starttls(bool)` |

`SmtpConfig` también implementa `Debug` a mano en lugar de derivarlo, específicamente para
que `auth`/`proxy` nunca impriman credenciales en texto plano si alguna vez se registra una
configuración en un log.

## Soporte en Node.js, Bun, Deno

`createTransport()` de `trustmailer-node` construye un `SmtpConfig` a partir de un objeto
plano de JS, pero hoy reenvía un conjunto de campos más reducido que el que admite Rust
nativo directamente:

| Campo en JS | Se traduce en |
|---|---|
| `service` | `SmtpConfig::for_service(service)` |
| `host` | `SmtpConfig::new(host, ..)` |
| `port` | `SmtpConfig::new(.., port)` (por defecto `587` si se omite y no hay `service`) |
| `secure` | `.with_secure(secure)` |
| `auth.user` / `auth.pass` | `.with_auth(Auth::Plain { username, password })` |
| `dkim.domainName` / `dkim.keySelector` / `dkim.privateKey` | envuelve el transporte con firma DKIM (no es específico de SMTP) |
| `sendmail` / `path` | cambia por completo al transporte sendmail, sin pasar por SMTP |
| `streamTransport` / `jsonTransport` | cambia por completo a un transporte que no envía nada |

**Todavía no expuesto a Node/Bun/Deno** (por ahora, solo Rust nativo):
`require_tls`, `ehlo_name`, `pool`/`pool_size`, `proxy`, autenticación OAuth2 (desde JS
hoy solo es alcanzable `Auth::Plain`), `connect_timeout`/`command_timeout`,
`allow_insecure_auth`, `disable_starttls`, y `from_url`. Si tu proyecto en Node.js
necesita alguno de estos, la vía para conseguirlo hoy es el crate de Rust nativo —
consulta la [guía rápida de Node.js](/es/docs/quickstart/nodejs/) y la
[guía rápida de Rust nativo](/es/docs/quickstart/native-rust/) para ambos puntos de
entrada, y la [matriz de runtimes](/es/docs/runtime-matrix/) para el panorama completo por
runtime.

:::tip[Las garantías de seguridad siguen aplicando]
Aunque los campos de STARTTLS/`allow_insecure_auth` todavía no se pueden configurar de
forma independiente desde JS, las protecciones subyacentes sí aplican: `createTransport()`
siempre construye un `SmtpConfig` con `disable_starttls: false` y
`allow_insecure_auth: false` (los valores por defecto seguros), así que tanto el upgrade
STARTTLS oportunista como la protección contra AUTH en texto plano se aplican exactamente
como se describe arriba, sin importar qué runtime esté llamando.
:::
