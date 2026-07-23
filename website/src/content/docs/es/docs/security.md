---
title: Seguridad
description: Qué se encontró, qué se corrigió y cómo se verificó — dos veces.
---

El sello en la marca no es decoración. Toda la propuesta de trustmailer es el modelo de
nodemailer sin tener que confiar en el árbol de dependencias transitivas de una librería
de correo en JS — así que la confianza hay que ganársela en las partes propensas a
inyección: el ensamblado de headers, las direcciones de sobre (envelope) y la firma DKIM.

## Encontrado portando los propios tests de regresión de nodemailer

Se encontraron y corrigieron dos clases de vulnerabilidad portando los tests de regresión
de seguridad de nodemailer, no solo sus tests de camino feliz — el tipo de bug que solo
aparece cuando deliberadamente adoptas la mirada del atacante:

**Inyección de headers MIME** — portado de `test/nodemailer/list-headers-test.js`, test de
regresión de [GHSA-268h-hp4c-crq3](https://github.com/advisories/GHSA-268h-hp4c-crq3):
cualquier valor de header que llevara un CRLF podía terminar ese header antes de tiempo e
inyectar headers adicionales arbitrarios en el mensaje. Corregido con una sanitización
universal en el único punto donde se escribe cada header (`mime/node.rs::write_header_line`),
no parcheado en cada sitio de uso.

**Inyección de comandos SMTP** — portado de `test/smtp-connection/smtp-connection-test.js`:
el nombre `EHLO` y las direcciones de sobre `MAIL FROM`/`RCPT TO` se escriben directamente
en comandos del protocolo SMTP. Un CRLF en el nombre EHLO se elimina en la asignación
(`SmtpConfig::with_ehlo_name`); un CRLF/`<`/`>` en una dirección de sobre se rechaza antes
de que un solo byte llegue al socket (`SmtpConnection::send_transaction`).

## Encontrado al ejecutarlo de verdad

Un tercer bug, este no de portar un test sino de ejecutar de punta a punta la ruta de
Cloudflare Workers en vivo: el valor de retorno JS de `sendMail()` llegaba como un `{}`
vacío tanto en `trustmailer-wasm` como en `trustmailer-smtp-cfworkers`. `serde-wasm-bindgen`
serializa los campos de struct como un `Map` de JS por defecto, y un `Map` no tiene
propiedades enumerables propias, así que `Response.json()`/`JSON.stringify()` lo
renderizaba silenciosamente como `{}` — sin error, solo datos silenciosamente incorrectos
llegando a quien llama. Corregido serializando con `serialize_maps_as_objects(true)` en
ambos crates. Una revisión solo de documentación o solo de compilación nunca habría
sacado esto a la luz — solo apareció porque la respuesta JSON real se inspeccionó de
punta a punta contra una ejecución real de `workerd`.

## Re-auditado de forma independiente

Después de la primera ronda de correcciones, una segunda auditoría independiente
re-verificó cada corrección y buscó específicamente lo que la primera pasada pudo haber
pasado por alto — tanto regresiones que las propias correcciones pudieran haber
introducido como cosas que ambas pasadas pasaran por alto. Encontró tres problemas más,
los tres ya corregidos y con test de regresión:

- Un **panic alcanzable en el parser**: una entrada de lista de direcciones que era solo
  un comentario (`(bar)` sin dirección precedente) desincronizaba dos vectores internos
  paralelos e indexaba fuera de rango — alcanzable desde cualquier valor `To:`/`Cc:`,
  violando el contrato de "el parser de direcciones nunca falla" por diseño.
- Una **guarda de OAuth2 evadible**: la restricción de solo-HTTPS en el endpoint de token
  OAuth2 tenía una excepción para loopback (`http://127.0.0.1...`) implementada como un
  `starts_with` simple, así que `http://127.0.0.1.evil.com` se colaba y habría enviado
  `client_secret`/`refresh_token` en texto plano a un host controlado por un atacante.
- Una **regresión de STARTTLS**: una corrección anterior hizo que la actualización a
  STARTTLS fuera incondicional siempre que un servidor la anunciara, sin forma de
  rechazarla — rompiendo conexiones a relays internos con un certificado no confiable que
  antes funcionaban en texto plano. Se añadió de vuelta un escape `disable_starttls`.

## Verificado, no asumido

- La **canonicalización DKIM** firma sobre bytes crudos, no texto decodificado con
  pérdida, y se comprueba contra el propio firmante de nodemailer real: la misma clave RSA
  y el mismo mensaje producen valores `bh=` y `b=` idénticos byte a byte — un test de
  verificación cruzada entre implementaciones real, no solo una aserción interna.
- **174 tests en 23 suites**, `cargo clippy --workspace --all-targets` limpio, verificado
  compilando en nativo, `wasm32-unknown-unknown` y el addon nativo de Node.js.
- La ruta SMTP de Cloudflare Workers está verificada contra una ejecución real de
  `wrangler dev`/`workerd` — ver la [guía rápida de Cloudflare Workers](/es/docs/quickstart/cloudflare-workers/)
  para los pasos de reproducción.

La auditoría completa y sin abreviar — cada hallazgo, cada file:line, cada corrección,
incluyendo los ítems de menor severidad dejados abiertos a propósito — es pública en
[`AUDIT.md`](https://github.com/nesgarbo/trustmailer/blob/main/AUDIT.md).
