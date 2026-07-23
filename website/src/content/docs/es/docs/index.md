---
title: Documentación
description: Documentación de trustmailer — librería de correo compatible con nodemailer, en Rust, para Rust nativo, Node.js/Bun/Deno, Cloudflare Workers y el navegador.
tableOfContents: false
---

Una librería de correo compatible con [nodemailer](https://nodemailer.com), hecha en Rust,
para todos los runtimes: Rust nativo, Node.js/Bun/Deno (un solo addon nativo, sin recompilar
por runtime), Cloudflare Workers (SMTP real sobre TCP saliente genuino, no un relay HTTP) y
el navegador/otros hosts WASM. Mismo modelo de mensaje, mismo contrato de transporte — un
solo núcleo en Rust, cuatro formas de invocarlo.

`trust` porque está hecho en Rust, y porque no deberías tener que confiar en el árbol de
dependencias transitivas de una librería de correo en JS con tus credenciales SMTP.

## Por dónde empezar

- **[Por qué](/es/docs/why/)** — el problema que resuelve trustmailer, y los cuatro crates que lo resuelven.
- **[Arquitectura](/es/docs/architecture/)** — cómo encajan los crates entre sí, y por qué el núcleo compila a `wasm32` sin tocarlo.
- **Guía rápida** — copia y pega `createTransport`/`sendMail` para [Rust nativo](/es/docs/quickstart/native-rust/), [Node/Bun/Deno](/es/docs/quickstart/nodejs/), [Cloudflare Workers](/es/docs/quickstart/cloudflare-workers/) y el [navegador](/es/docs/quickstart/browser-wasm/).
- **[Matriz de soporte por runtime](/es/docs/runtime-matrix/)** — qué transporte funciona en qué runtime exactamente, y por qué los huecos son arquitectónicos, no funcionalidades faltantes.
- **[Seguridad](/es/docs/security/)** — qué se encontró, qué se corrigió y cómo se verificó.
- **[Compilación](/es/docs/building/)** — comandos de build para cada target.

El código fuente, los issues y el log de auditoría completo viven en [GitHub](https://github.com/nesgarbo/trustmailer).
