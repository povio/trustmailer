---
title: Arquitectura
description: Cómo encajan los crates de trustmailer entre sí, y por qué el núcleo compila a wasm32 sin tocarlo.
---

```
trustmailer-core           modelo de mensaje, parseo de direcciones, builder MIME, trait Transport
  ├─ trustmailer-smtp-protocol  lógica SMTP pura (comandos/respuestas/dot-stuffing) [cualquier runtime]
  │    ├─ trustmailer-smtp        SMTP sobre TCP de tokio (TLS/STARTTLS, auth, pool, proxy) [nativo]
  │    └─ trustmailer-smtp-cfworkers  SMTP sobre cloudflare:sockets connect()          [Cloudflare Workers]
  ├─ trustmailer-sendmail   envía por tubería a un binario `sendmail` local            [nativo]
  ├─ trustmailer-ses        API HTTPS de Amazon SES v2, SigV4 hecho a mano             [cualquier runtime]
  ├─ trustmailer-dkim       firma DKIM (RSA-SHA256, relaxed/relaxed)                    [cualquier runtime]
  ├─ trustmailer-stream     renderiza el MIME crudo en vez de enviarlo (tests, relays propios) [cualquier runtime]
  └─ trustmailer-json       renderiza un mensaje JSON en vez de enviarlo (tests)        [cualquier runtime]

trustmailer              crate fachada: re-exporta el núcleo + todos los transportes nativos, tras features
trustmailer-node         binding napi-rs — createTransport()/sendMail() para Node.js/Bun/Deno
trustmailer-wasm         binding wasm-bindgen — createTransport()/sendMail() para navegador/otros hosts WASM
```

`trustmailer-core` y `trustmailer-smtp-protocol` no hacen I/O y compilan a `wasm32` tal
cual: el parseo de direcciones, la construcción MIME y el manejo de comandos/respuestas
SMTP son funciones puras sobre bytes/strings, compartidas por cada binding — solo el
streaming de bytes (`tokio` TCP vs. `cloudflare:sockets` vs. nada) cambia según el
transporte.

:::note[Composable, no monolítico]
La firma DKIM es un decorador de `Transport` — `SigningTransport::new(signer, inner_transport)` —
así que envuelve SMTP, SES, sendmail o stream/JSON de forma transparente, en cualquier
runtime capaz de compilar `trustmailer-dkim`.
:::

Consulta la [matriz de soporte por runtime](/es/docs/runtime-matrix/) para ver exactamente
qué transporte está disponible en qué runtime, y por qué.
