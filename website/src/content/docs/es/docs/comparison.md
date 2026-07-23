---
title: nodemailer vs trustmailer
description: Una comparación real y reproducible — paridad de salida, un test de regresión de inyección CRLF, rendimiento, y las advertencias publicadas actuales de nodemailer.
---

Cada afirmación de esta página viene de un script en el directorio
[`benchmarks/`](https://github.com/nesgarbo/trustmailer/tree/main/benchmarks) del
repositorio, ejecutado contra el paquete `nodemailer` real, publicado actualmente — no una
copia guardada, no una versión antigua elegida para quedar mal. Clona el repositorio y
ejecuta `cd benchmarks && npm install && npm run parity && npm run bench` para obtener tus
propios números; variarán según el hardware, pero la forma del resultado no.

## Seguridad: las advertencias actuales de nodemailer

Ejecutar `npm install nodemailer` hoy (esta página se regeneró por última vez contra
**nodemailer 9.0.3**, la versión más reciente) y luego `npm audit` saca a la luz seis
advertencias de severidad alta, actualmente abiertas, que afectan a nodemailer `<=9.0.0`
— corregidas solo en la línea 9.0.1+ recién publicada:

| Advertencia | Problema |
| --- | --- |
| [GHSA-c7w3-x93f-qmm8](https://github.com/advisories/GHSA-c7w3-x93f-qmm8) | Inyección de comandos SMTP vía `envelope.size` sin sanitizar |
| [GHSA-vvjj-xcjg-gr5g](https://github.com/advisories/GHSA-vvjj-xcjg-gr5g) | Inyección de comandos SMTP vía CRLF en el nombre de transporte (EHLO/HELO) |
| [GHSA-268h-hp4c-crq3](https://github.com/advisories/GHSA-268h-hp4c-crq3) | Inyección CRLF en comentarios de headers `List-*` — inyección arbitraria de headers |
| [GHSA-wqvq-jvpq-h66f](https://github.com/advisories/GHSA-wqvq-jvpq-h66f) | `jsonTransport` evade `disableFileAccess`/`disableUrlAccess` durante la normalización |
| [GHSA-r7g4-qg5f-qqm2](https://github.com/advisories/GHSA-r7g4-qg5f-qqm2) | Validación incorrecta del certificado TLS durante la obtención del token OAuth2 |
| [GHSA-p6gq-j5cr-w38f](https://github.com/advisories/GHSA-p6gq-j5cr-w38f) | La opción `raw` a nivel de mensaje evade las restricciones de acceso a archivo/URL — lectura arbitraria de archivos y SSRF |

Esa tercera, GHSA-268h-hp4c-crq3, es exactamente la clase de inyección de headers vía
CRLF que trustmailer portó como test de regresión desde la propia suite `test/` de
nodemailer (ver [Seguridad](/es/docs/security/)) — el mismo tipo de bug, encontrado de
forma independiente, en ambos lados de esta comparación.

:::note[Esto no es "nodemailer es malo"]
Nodemailer corrigió las seis en la línea 9.0.1+ — si tienes una instalación actual, estás
bien. El punto no es que nodemailer esté mal mantenido; es que ensamblar comandos SMTP y
headers MIME a mano en un lenguaje de tipado dinámico ha producido seis advertencias
reales a lo largo de su historia, precisamente la clase de bug que un núcleo en Rust con
sanitización explícita en un único punto de paso (`mime/node.rs::write_header_line`) es
más difícil de reintroducir por accidente. Si ese compromiso te importa es una decisión
real, no una conclusión inevitable — esta página solo pone números reales al lado.
:::

## Paridad de salida

[`parity.cjs`](https://github.com/nesgarbo/trustmailer/blob/main/benchmarks/parity.cjs)
construye el mismo mensaje idéntico (`from`/`to`/`cc`/`subject`/`text`/`html`) con ambas
librerías y comprueba las partes que deberían coincidir de verdad — cantidad de
destinatarios, subject, y cuerpos de texto y HTML vuelven idénticos, verificados campo por
campo, no a ojo.

También pasa el payload de inyección CRLF de GHSA-268h-hp4c-crq3 por **las rutas de
código reales de ambas librerías reales** y confirma que ninguna forja un segundo header
— pero solo a través del transporte que realmente construye headers MIME
(`streamTransport` en ambos lados). `jsonTransport` no sanitiza CRLF en **ninguna** de
las dos librerías, porque ninguna construye headers reales ahí — solo serializa los
valores de los campos crudos para inspección. Eso no es un hueco específico de
trustmailer; es un comportamiento idéntico confirmado en ambos lados, y el script también
lo afirma en vez de esconderlo.

```
7/7 checks passed
```

Una asimetría que el script tuvo que sortear, y que vale la pena decir claramente: a día
de hoy, el `sendMail()` de `trustmailer-node` no devuelve el contenido JSON/MIME
realmente renderizado para `jsonTransport`/`streamTransport` como sí hace el
`info.message` de nodemailer — solo un string de confirmación fijo (ver la documentación
de los transportes [Stream](/es/docs/transports/stream/) y [JSON](/es/docs/transports/json/)).
`parity.cjs` lo sortea llamando directamente al crate de Rust real para el lado de
trustmailer en la comparación. Es un hueco real y abierto en el binding de Node.js, no
está escondido bajo la alfombra.

## Rendimiento

[`bench.cjs`](https://github.com/nesgarbo/trustmailer/blob/main/benchmarks/bench.cjs)
construye y renderiza el mismo lote de mensajes (`jsonTransport`, así que no hay red de
por medio en ninguno de los dos lados) con el `sendMail()` real de cada librería,
cronometrado con `process.hrtime.bigint()`, tras una pasada de calentamiento. Tres
ejecuciones separadas de 20.000 mensajes cada una, en la misma máquina, una tras otra:

| Ejecución | nodemailer | trustmailer | Ratio |
| --- | ---: | ---: | ---: |
| 1 | 10.159 msg/s | 15.767 msg/s | 1.55× |
| 2 | 9.866 msg/s | 20.819 msg/s | 2.11× |
| 3 | 9.908 msg/s | 15.718 msg/s | 1.59× |

Aproximadamente **1.5–2×** el rendimiento de nodemailer para construir y renderizar un
mensaje, de forma consistente entre ejecuciones — con variación real de una ejecución a
otra, reportada honestamente en vez de elegir el mejor número. Esto mide solo
construcción/renderizado de mensajes (sin I/O de red en ningún lado); no dice nada sobre
el rendimiento real de entrega SMTP, que está dominado por el servidor remoto y la red,
no por la librería cliente.

## Huella de instalación

También medido directamente, no estimado:

| | nodemailer | trustmailer (Node.js) |
| --- | --- | --- |
| Dependencias npm en tiempo de ejecución | **0** | **0** |
| Tamaño en disco | 676 KB (JS plano) | ~20 MB (un binario nativo compilado) |

Ambas están honestamente libres de dependencias a nivel de npm — esta fue una de las
pocas afirmaciones de esta comparación que resultó ser un empate en vez de una victoria,
y se reporta así. La diferencia de tamaño no es un riesgo de cadena de suministro en
ningún lado; es una diferencia de modelo de distribución. nodemailer distribuye JS
interpretado sin stack de cripto/TLS/HTTP incluido (delega el TLS al propio módulo `tls`
de Node). trustmailer distribuye un binario compilado que enlaza estáticamente su propio
TLS (`rustls`), cliente HTTP (`reqwest`), e implementaciones de RSA/SHA (`rsa`, `sha2`) —
el costo del compromiso por la seguridad de memoria y las clases de CVE de arriba, no
algo gratis.

## Qué no se compara aquí

- **Paridad de funcionalidades** en general — ver [Por qué](/es/docs/why/) y
  [Mensaje](/es/docs/message/) para qué está portado y qué no lo está a propósito (DSN,
  `attachDataUrls`, plugins de autenticación personalizados, y más — ver los avisos de
  cada página).
- **Soporte multi-runtime** — nodemailer es solo-Node por diseño; esta página es una
  comparación en el mismo runtime. Ver la [matriz de soporte por runtime](/es/docs/runtime-matrix/)
  para lo que trustmailer añade más allá de Node.js.
- **Rendimiento real de entrega SMTP** — el rendimiento de envío real de ambas librerías
  está limitado por el servidor de destino y la red, no medido aquí.

## Reprodúcelo tú mismo

```sh
git clone https://github.com/nesgarbo/trustmailer
cd trustmailer
cargo build --release --example render_json --example render_stream -p trustmailer
cd crates/trustmailer-node && npx napi build --platform --release && cd ../..
cd benchmarks
npm install
npm run parity
npm run bench
```
