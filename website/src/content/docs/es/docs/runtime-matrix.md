---
title: Matriz de soporte por runtime
description: Exactamente qué transporte funciona en qué runtime, y por qué.
---

| Transporte            | Rust nativo | Node · Bun · Deno | Cloudflare Workers                                       | Navegador / otro WASM                         |
| ---------------------- | :---------: | :----------------: | :---------------------------------------------------------- | :----------------------------------------------- |
| SMTP (TLS/STARTTLS)    | ✅          | ✅                  | ✅ — TCP real vía `cloudflare:sockets`, no un relay          | ❌¹ — sin sockets TCP crudos en una página normal |
| Sendmail               | ✅          | ✅                  | ❌² — sin lanzamiento de procesos en el isolate del Worker    | ❌ — sin lanzamiento de procesos, punto final     |
| SES (API HTTPS)        | ✅          | ✅                  | ✅                                                            | ✅ — `reqwest` usa `fetch` bajo `wasm32`          |
| Stream / JSON          | ✅          | ✅                  | ✅                                                            | ✅                                                 |
| Firma DKIM             | ✅ (`SigningTransport` envuelve cualquier transporte) | ✅ (opción `dkim` en `createTransport`) | ✅ (se compone con `trustmailer-dkim` directamente) | ✅ |

¹ Comprobado, no asumido: Chrome sí tiene una
[Direct Sockets API](https://developer.chrome.com/docs/iwa/direct-sockets) de TCP crudo,
pero está limitada a **Isolated Web Apps** — un modelo de app empaquetada, firmada e
instalada por separado, no una página cargada en una URL en una pestaña normal — así que
no aplica al "navegador" en el sentido que apunta este crate (una librería que importa una
página web). Ningún otro navegador ofrece algo equivalente.

² Comprobado, no asumido: el isolate del Worker genuinamente no puede lanzar un proceso —
la propia documentación de Cloudflare es explícita en que los Workers corren en un isolate
de V8 sin disco y sin forma de lanzar un proceso nativo.
[Cloudflare Containers](https://developers.cloudflare.com/containers/) (un producto
distinto — contenedores Linux reales, con soporte de `exec()`) sí puede correr un binario
`sendmail` real, pero es un primitivo de cómputo distinto, no algo que el código del
isolate de Worker de `trustmailer-smtp-cfworkers` pueda tocar.

:::tip[Verifícalo tú mismo]
Cloudflare Workers obtienen SMTP real, no un relay — los pasos de reproducción están en la
página de [guía rápida de Cloudflare Workers](/es/docs/quickstart/cloudflare-workers/). Un
proyecto real de `wrangler dev` envía un mensaje MIME completo sobre una transacción SMTP
TCP real a un servidor SMTP falso local.
:::

Ambos huecos de arriba son arquitectónicos, no funcionalidades faltantes — ver
[Seguridad](/es/docs/security/) para cómo se prueba y verifica el resto de la superficie
de transporte.
