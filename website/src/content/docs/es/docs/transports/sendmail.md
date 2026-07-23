---
title: Transporte Sendmail
description: Enviar el correo compuesto por la entrada estándar de un binario compatible con sendmail — sin shell, sin red.
---

El transporte sendmail entrega tu mensaje compuesto a un binario local compatible con
`sendmail`, en lugar de hablar SMTP directamente. Es el patrón de entrega clásico de
Unix: un agente de transporte de correo (Postfix, Exim, OpenSMTPD, msmtp...) instala un
shim `/usr/sbin/sendmail`, y cualquier cosa que escriba un mensaje RFC 5322 en la
entrada estándar de ese shim obtiene encolado local, reenvío y reintentos gratis. Está
implementado en `trustmailer-sendmail`, reexportado desde el crate fachada como
`trustmailer::sendmail` tras la feature de Cargo `sendmail` (activada por defecto).

Este transporte lanza un proceso real del sistema operativo, así que solo existe en
runtimes nativos — no está disponible desde `trustmailer-wasm` (los navegadores y
Cloudflare Workers no pueden lanzar procesos), y compilar `trustmailer` para
`wasm32-unknown-unknown` con la feature `sendmail` activada falla a propósito en tiempo
de compilación. Consulta la [matriz de soporte por runtime](/es/docs/runtime-matrix/)
para el panorama completo.

## Configuración

`SendmailTransport` se construye con `SendmailTransport::new()` y dos métodos
constructor opcionales:

| Campo  | Tipo          | Por defecto                     | Descripción |
| ------ | ------------- | -------------------------------- | ----------- |
| `path` | `String`      | `/usr/sbin/sendmail`              | Ruta al binario compatible con sendmail, se define con `.with_path(path)`. En general debería ser absoluta — un valor relativo cae a una búsqueda en `$PATH`, que casi nunca es lo que quieres en producción. |
| `args` | `Vec<String>` | `["-i", "-t"]`                    | Lista completa de argumentos de línea de comandos pasados al binario, se define con `.with_args(args)`. **Reemplaza** por completo el `-i -t` por defecto en lugar de añadirse a él — si llamas a `.with_args(...)`, eres tú quien tiene que incluir `-t` si aún quieres que los destinatarios se lean de las cabeceras del mensaje. |

`-i` le dice a sendmail que no trate un `.` solo en una línea como fin de entrada, y
`-t` le dice que tome los destinatarios de las cabeceras `To`/`Cc`/`Bcc` del mensaje en
vez de argv. Ambos son convenciones, no algo que trustmailer imponga — como
`.with_args()` reemplaza la lista por completo, puedes pasar el argv que tu MTA local
espere (por ejemplo `-f` para forzar una dirección de sobre-remitente distinta).

:::caution[`path`/`args` son configuración del operador, no entrada de un atacante]
`path` y `args` los define quien configura el transporte en el código — nunca se derivan
del contenido del mensaje, de las direcciones de remitente/destinatario ni de nada que
controle un usuario final. No los construyas a partir de entrada no confiable; trátalos
igual que una cadena de conexión a base de datos o una URL de API.
:::

## Cómo envía

`SendmailTransport::send()`:

1. Renderiza el mensaje a bytes RFC 5322 crudos con el mismo `mime::build()` que usan
   todos los demás transportes, con el hostname del Message-ID fijado a `localhost` (no
   hay un `with_hostname()` en este transporte como sí lo hay en el [transporte
   stream](/es/docs/transports/stream/) — si necesitas un dominio específico en el
   Message-ID, define `.message_id(...)` explícitamente en el `Message`).
2. Lanza `path` con `args` vía `tokio::process::Command`, con stdin, stdout y stderr
   todos redirigidos. Es una llamada `Command::new(path).args(args)` de argv puro — no
   hay ningún shell de por medio, así que no existe superficie de inyección por
   metacaracteres de shell ni en principio.
3. Escribe el mensaje renderizado en la entrada estándar del proceso hijo y la cierra
   (para que el hijo vea EOF), mientras drena **al mismo tiempo** la salida estándar y
   el error estándar.
4. Espera a que el proceso termine. Un código de salida distinto de cero se reporta
   como error, incluyendo el texto capturado de stderr.

La concurrencia del paso 3 no es incidental. Una versión anterior de este transporte
escribía toda la entrada estándar hasta completarse y solo *después* leía stdout/stderr
(al estilo `wait_with_output()`). Un binario sendmail — o un envoltorio verboso
alrededor de uno — puede llenar el búfer de la tubería del sistema operativo con salida
de stdout/stderr antes de haber drenado un cuerpo grande de stdin (un mensaje con
adjuntos considerables, por ejemplo), y escribir-y-luego-leer en secuencia estricta
puede bloquear ambos lados para siempre, sin ningún timeout. La corrección escribe
stdin y lee stdout/stderr dentro del mismo `tokio::join!`, así que ningún lado puede
bloquear al otro.

:::note[Una clase de bug de interbloqueo de tuberías, ya corregida]
Si estás evaluando trustmailer frente a otras implementaciones que invocan sendmail,
esto es exactamente lo que hay que revisar: ¿lee la salida del hijo *mientras* escribe
stdin, o solo después? Escribir-y-luego-leer secuencialmente es correcto para mensajes
pequeños y un sendmail silencioso, y se cuelga silenciosamente para mensajes grandes.
La suite de pruebas de `trustmailer-sendmail` lanza procesos reales de `/bin/cat` y
`/bin/sh` (no mocks) como sustitutos de sendmail para ejercitar esto.
:::

## Forma de `SentInfo`

- `accepted` — todos los destinatarios del sobre (`message.effective_envelope()?.to`).
  Sendmail no da forma de distinguir "aceptado" de "rechazado" por destinatario en el
  límite de la tubería, así que este transporte no puede rellenar `rejected` — aquí
  siempre está vacío; un problema de entrega aparece como un código de salida distinto
  de cero y un `Err`, no como una lista de rechazados.
- `response` — lo que sea que el binario haya impreso en stdout, normalmente nada (los
  binarios sendmail reales suelen ser silenciosos cuando tienen éxito).
- `raw` — siempre `None`. El mensaje renderizado se envió directamente a la entrada
  estándar del hijo y nunca se retuvo en memoria después, a diferencia de los
  transportes stream/JSON.
- `message_id` — el propio campo `message_id` del `Message` si definiste uno, `""` en
  caso contrario. Esto es una sutileza real: el MIME *renderizado* siempre obtiene una
  cabecera `Message-ID` generada (vía el paso 1 anterior) incluso cuando no definiste
  una, pero `SentInfo.message_id` solo refleja lo que había en la estructura `Message`
  de entrada. Si quieres recuperar un `message_id` no vacío, llama tú mismo a
  `.message_id(...)` en el builder.

## Ejemplo en Rust

```rust
use trustmailer::{Message, Transport};
use trustmailer::sendmail::SendmailTransport;

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let transport = SendmailTransport::new()
        .with_path("/usr/sbin/sendmail")
        .with_args(vec!["-i".into(), "-t".into(), "-f".into(), "bounces@example.com".into()]);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .message_id("<unique-id@example.com>")
        .build();

    let info = transport.send(&message).await?;
    println!("accepted: {:?}", info.accepted);
    Ok(())
}
```

## Node.js, Bun, Deno

`createTransport()` de `trustmailer-node` expone este transporte con `sendmail: true` y
un `path` opcional. Actualmente no hay forma de sobreescribir `args` desde el binding de
Node — ese método constructor solo existe en el `SendmailTransport` de Rust, así que si
necesitas argv personalizado, llama directamente al crate de Rust o abre un issue.

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  sendmail: true,
  path: "/usr/sbin/sendmail", // opcional, es el valor por defecto
});

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});
console.log("accepted:", info.accepted);
```

A diferencia de llamar directamente al crate de Rust, aquí `info.messageId` nunca está
vacío: el binding de Node pre-genera un `Message-ID` antes de llamar al transporte
cuando quien lo invoca no proporcionó uno, así que `SentInfoJs.message_id` siempre
refleja un ID real.

## Ver también

- [Transporte SES](/es/docs/transports/ses/) — entrega vía API HTTPS en lugar de un
  binario local.
- [Transporte Stream](/es/docs/transports/stream/) y
  [Transporte JSON](/es/docs/transports/json/) — sin entrega alguna, para pruebas y
  relays propios.
- [Matriz de soporte por runtime](/es/docs/runtime-matrix/) — por qué este transporte
  es solo nativo.
- [Seguridad](/es/docs/security/) — cómo se previene la inyección de cabeceras/
  direcciones antes de que el mensaje llegue siquiera a este transporte.
