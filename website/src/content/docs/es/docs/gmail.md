---
title: Usando Gmail
description: Enviar a través de Gmail/Google Workspace con una Contraseña de aplicación u OAuth2.
---

Gmail es, en la práctica, el destino más común para el transporte SMTP, y también el que
más probablemente rechace una configuración ingenua — Google dejó de aceptar una
contraseña de cuenta normal para el login SMTP hace años. Esta página es el camino
práctico para "hacer que funcione de verdad", construido sobre la
[referencia del transporte SMTP](/es/docs/transports/smtp/).

## El atajo de servicio conocido

`SmtpConfig::for_service("gmail")` resuelve a los valores exactos que Google publica para
el envío SMTP, verificados desde la propia tabla `well_known.rs` de trustmailer-core:

| | |
| --- | --- |
| Host | `smtp.gmail.com` |
| Puerto | `465` |
| Secure | `true` (TLS implícito) |

```rust
use trustmailer::smtp::{Auth, SmtpConfig, SmtpTransport};

let config = SmtpConfig::for_service("gmail")
    .expect("gmail es un servicio conocido")
    .with_auth(Auth::Plain {
        username: "tu@gmail.com".into(),
        password: "tu-contraseña-de-aplicación-de-16-caracteres".into(),
    });
let transport = SmtpTransport::new(config);
```

`for_service` también reconoce `"googlemail"` (un alias) y, por separado,
`"gmailworkspace"`/`"googleworkspacemail"` — esa variante apunta a
`smtp-relay.gmail.com`, el endpoint de *relay* de Google Workspace (un producto distinto
del SMTP de Gmail para consumidores, pensado para uso de relay autenticado por dominio,
no un sustituto de elegir el método de autenticación de abajo).

## Autenticación: Contraseña de aplicación vs. OAuth2

La contraseña de la cuenta de Google no funciona para `AUTH PLAIN`/`LOGIN` de SMTP —
nunca lo ha hecho, para clientes SMTP de terceros, una vez que la verificación en 2 pasos
está activada (y Google lleva tiempo exigiendo la verificación en 2 pasos en toda la
cuenta). Hay dos opciones reales:

### Contraseña de aplicación (la más simple, cuentas personales)

Requiere tener activada la verificación en 2 pasos en la cuenta de Google primero.
Genera una en
[myaccount.google.com/apppasswords](https://myaccount.google.com/apppasswords) — es una
cadena de 16 caracteres, formateada con espacios, limitada solo a este uso. Úsala tal cual
como `password` en `Auth::Plain`, como se muestra arriba. Es el camino más rápido para una
cuenta de Gmail personal, y coincide con cómo los usuarios de nodemailer suelen configurar
Gmail también.

:::caution[No siempre disponible]
Los administradores de Google Workspace pueden desactivar las Contraseñas de aplicación
para todo el dominio. Si la generación de contraseñas aparece deshabilitada o ausente en
la configuración de seguridad de una cuenta Workspace, OAuth2 es el único camino que
queda — ver abajo.
:::

### OAuth2 (amigable con Workspace, sin secreto compartido)

El `Auth::XOAuth2` de trustmailer más `OAuth2Config`/`OAuth2TokenProvider` (documentado por
completo en la [página del transporte SMTP](/es/docs/transports/smtp/#oauth2-xoauth2))
maneja el flujo de refresco de token OAuth2 de Google:

```rust
use trustmailer::smtp::{OAuth2Config, OAuth2TokenProvider, SmtpConfig, SmtpTransport};

let oauth2 = OAuth2TokenProvider::new(OAuth2Config::new(
    "tu@gmail.com",
    "....apps.googleusercontent.com", // client_id
    "...",                            // client_secret
    "...",                            // refresh_token
));
// OAuth2Config::new() ya usa por defecto el endpoint de token de Google como
// access_url — sobreescríbelo con .with_access_url(...) solo para un proveedor
// que no sea Google.

let config = SmtpConfig::for_service("gmail")
    .expect("gmail es un servicio conocido")
    .with_auth(oauth2.to_auth().await?);
let transport = SmtpTransport::new(config);
```

Obtener un trío `client_id`/`client_secret`/`refresh_token` de Google OAuth2 es una tarea
de configuración en Google Cloud Console (activar la API de Gmail, crear credenciales
OAuth, ejecutar el flujo de consentimiento una vez para generar un refresh token) — esa
parte es idéntica a lo que la propia guía de OAuth2 para Gmail de nodemailer explica, y
está fuera del alcance de lo que controla el código de trustmailer.

## Node.js

```js
const { createTransport } = require("trustmailer");

const transporter = createTransport({
  host: "smtp.gmail.com",
  port: 465,
  secure: true,
  auth: { user: "tu@gmail.com", pass: "tu-contraseña-de-aplicación-de-16-caracteres" },
});

const info = await transporter.sendMail({
  from: "tu@gmail.com",
  to: "bob@example.com",
  subject: "Hola desde Gmail",
  text: "Enviado con trustmailer.",
});
```

Hoy no hay un atajo `for_service`/búsqueda-de-servicio-conocido expuesto al binding de
Node.js — pasa `smtp.gmail.com`/`465`/`true` directamente, como arriba.

## Fallo común

Una contraseña de cuenta normal (no una Contraseña de aplicación, no un token OAuth2) en
`auth.pass` falla la autenticación — trustmailer lo expone como
[`Error::Transport`](/es/docs/errors/#transportstring) con la razón de rechazo del
servidor en el mensaje, la misma clase de fallo que nodemailer llama `EAUTH`. Si ves un
fallo de autenticación contra `smtp.gmail.com`, el campo de contraseña es casi siempre lo
primero que hay que revisar.
