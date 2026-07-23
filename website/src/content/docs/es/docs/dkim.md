---
title: Firma DKIM
description: Cómo firma trustmailer el correo saliente con DKIM, y qué se ha verificado exactamente al respecto.
---

DKIM (DomainKeys Identified Mail, RFC 6376) permite que un servidor de correo receptor
confirme que un mensaje realmente fue enviado por quien dice haberlo enviado, y que nada
en él fue alterado en tránsito. trustmailer firma con RSA-SHA256 sobre canonicalización
relaxed/relaxed — la misma combinación que usa nodemailer por defecto — implementado en
el crate `trustmailer-dkim` como un firmante pequeño y autocontenido, más un decorador de
`Transport` que envuelve el transporte que ya estés usando.

Como esta es la única parte del código cuyo trabajo entero es la corrección
criptográfica, [`AUDIT.md`](https://github.com/nesgarbo/trustmailer/blob/main/AUDIT.md)
la trata con mayor prioridad que hallazgos etiquetados de forma similar en el resto del
proyecto, y pasó por dos rondas independientes de auditoría. Los recuadros de abajo están
tomados de esa auditoría, no de texto de marketing — donde algo se encontró y se
corrigió, aquí se dice explícitamente.

## Cómo funciona la firma

Firmar ocurre en cuatro pasos, todos dentro de `DkimSigner::sign`:

1. **Dividir** el mensaje RFC 5322 crudo en cabeceras y cuerpo, desplegando de paso
   cualquier línea de continuación de cabecera.
2. **Canonicalizar** ambos, usando el algoritmo *relaxed* de RFC 6376 §3.4.2/§3.4.4: los
   nombres de cabecera se pasan a minúsculas, las secuencias de espacios/tabs colapsan a
   un solo espacio, y se recorta el espacio en blanco al inicio/final; al cuerpo se le
   quita el espacio en blanco final de cada línea y se descartan las líneas en blanco
   finales, terminando siempre en exactamente un CRLF.
3. **Hashear** el cuerpo canonicalizado con SHA-256 para obtener la etiqueta `bh=`,
   luego construir la entrada de firma completa (cabeceras canonicalizadas firmadas más
   la propia cabecera DKIM-Signature, con el valor `b=` aún ausente) y hashear *eso*.
4. **Firmar** ese segundo hash con RSA-PKCS#1v1.5, codificar la firma en base64 dentro de
   `b=`, y anteponer la cabecera `DKIM-Signature:` terminada al mensaje.

:::note[A nivel de bytes, no de texto]
Ambos pasos de canonicalización operan directamente sobre los bytes crudos del mensaje —
no hay ningún paso de decodificación `String`/`char` en todo el camino. Eso importa para
cualquier cuerpo con contenido de 8 bits heredado (texto `iso-8859-1`/`windows-1252`, una
codificación de transferencia cruda/binaria): decodificar primero de forma lossy a través
de UTF-8 mutaría esos bytes silenciosamente antes de hashear, produciendo un `bh=` que no
coincide con lo que calcula el servidor receptor sobre los bytes reales de la conexión —
una firma que se verifica como rota, sin ningún error mostrado jamás al remitente. Una
versión anterior de este crate sí decodificaba con `String::from_utf8_lossy` antes de
hashear; se encontró y se reescribió para operar sobre bytes crudos de principio a fin, y
la corrección se rastreó a mano contra RFC 6376 §3.4.4 en cada caso límite (cuerpo vacío,
sin salto de línea final, líneas en blanco internas, CR suelto, bytes no-UTF-8, cabeceras
plegadas/con relleno) en una segunda ronda de verificación independiente.
:::

## Inicio rápido (Rust nativo)

```rust
use trustmailer::{Message, Transport};
use trustmailer::dkim::{DkimSigner, SigningTransport};
use trustmailer::smtp::{SmtpConfig, SmtpTransport};

#[tokio::main]
async fn main() -> trustmailer::Result<()> {
    let private_key_pem = std::fs::read_to_string("dkim-private.pem")?;
    let signer = DkimSigner::from_pkcs8_pem("example.com", "default", &private_key_pem)?;

    let smtp = SmtpTransport::new(SmtpConfig::for_service("gmail").unwrap());
    let transport = SigningTransport::new(signer, smtp);

    let message = Message::builder()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .subject("Hello")
        .text("Hi Bob!")
        .build();

    let info = transport.send(&message).await?;
    println!("{:?}", info);
    Ok(())
}
```

`SigningTransport` implementa el mismo trait `Transport` que todo lo demás, así que
compone con *cualquier* transporte interior — SMTP, sendmail, stream, JSON — y quien esté
del otro lado nunca necesita saber que se está firmando.

## Inicio rápido (Node.js)

`createTransport` de `trustmailer-node` acepta una opción `dkim` con la misma forma que
la de nodemailer, y conecta un `SigningTransport` delante del transporte que hayas
configurado:

```js
import { createTransport } from "trustmailer";
import { readFileSync } from "node:fs";

const transporter = createTransport({
  service: "gmail",
  auth: { user: "user@gmail.com", pass: "app-password" },
  dkim: {
    domainName: "example.com",
    keySelector: "default",
    privateKey: readFileSync("dkim-private.pem", "utf8"),
  },
});

const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "bob@example.com",
  subject: "Hello",
  text: "Hi Bob!",
});
```

## Referencia de configuración

`DkimSigner` se construye con un constructor más un pequeño número de métodos builder —
no existe un "objeto de opciones" separado, así que cada fila de abajo corresponde a una
llamada en Rust (y, donde se indica, a un campo JS equivalente en la opción `dkim` del
binding de Node):

| Rust                                             | Campo `dkim` en Node | Obligatorio | Propósito |
| ------------------------------------------------- | -------------------- | :---------: | --------- |
| `DkimSigner::from_pkcs8_pem(domain, selector, pem)` | —                   | sí          | Construye el firmante. `domain` se convierte en la etiqueta `d=`, `selector` en `s=`, y `pem` debe ser una clave privada RSA codificada en PKCS#8 PEM. |
| — (primer argumento de arriba)                    | `domainName`          | sí          | El dominio firmante, publicado en la etiqueta `d=` y consultado como `<selector>._domainkey.<domain>` para obtener la clave pública. |
| — (segundo argumento de arriba)                   | `keySelector`         | sí          | El selector DKIM, publicado en la etiqueta `s=`. |
| — (tercer argumento de arriba)                    | `privateKey`          | sí          | Texto de la clave privada codificada en PKCS#8 PEM. |
| `.with_headers(Vec<String>)`                       | — (aún no expuesto)   | no          | Reemplaza la lista predeterminada de cabeceras a firmar (ver abajo). Sin distinguir mayúsculas/minúsculas; se pasa a minúsculas internamente. |
| `.key_bits()`                                      | —                     | no          | Informativo — devuelve el tamaño de la clave RSA en bits, por ejemplo para advertir sobre claves de menos de 1024 bits. |
| `SigningTransport::new(signer, inner)`             | (implícito al fijar `dkim`) | sí   | Envuelve `inner` para que cada `send()` firme primero. |
| `.with_hostname(String)`                           | — (aún no expuesto)   | no          | Nombre de host usado al generar un `Message-ID` para mensajes que no traen uno. Por defecto `"localhost"`. |

## Elegir qué cabeceras se firman

Por defecto, `DkimSigner` firma las que estén presentes de una lista fija recomendada por
RFC 4871 §5.5 — `From`, `Sender`, `Reply-To`, `Subject`, `Date`, `Message-ID`, `To`, `Cc`,
cabeceras MIME/de contenido, `Resent-*`, `In-Reply-To`, `References`, y las cabeceras
`List-*` — siempre firmadas en ese orden de prioridad fijo en lugar del orden en que
aparecen en el mensaje, de modo que dos firmantes con la misma clave y mensaje producen
firmas idénticas byte a byte.

Llama a `.with_headers([...])` para reemplazar esa lista por completo con la tuya propia
— es una **lista explícita de firma**, no una lista de exclusión: lo que pases es
exactamente lo que se busca y se firma, en el orden que le des. Si una cabecera que
nombras no está presente en el mensaje, simplemente se omite, no es un error. Si un
mensaje tiene más de una cabecera con el mismo nombre, se firma la más cercana al cuerpo.

:::caution[`From` debe estar en el conjunto firmado]
`sign()` falla con `Error::Dkim("From header must be signed")` si `From` no termina
entre las cabeceras firmadas — ya sea porque el mensaje no tiene cabecera `From` en
absoluto, o porque una lista personalizada con `.with_headers([...])` la dejó fuera. Esta
es una comprobación deliberada y separada de la comprobación general de "al menos una
cabecera se firmó": una firma que cubra alguna cabecera pero no `From` se validaría en el
receptor sin decir nada confiable sobre quién realmente envió el mensaje — justo la
propiedad que DKIM existe para garantizar. Una versión anterior de este crate solo
comprobaba que "el conjunto firmado no esté vacío", algo que una llamada a
`.with_headers([...])` que omitiera `"from"` podía superar dejando sin firmar la propia
identidad del remitente; ese hueco se encontró y se cerró añadiendo esta comprobación
dedicada.
:::

## El dominio y el selector se validan, no se confían a ciegas

`DkimSigner::from_pkcs8_pem` rechaza cualquier `domain` o `selector` que contenga un CR,
LF, `;` o `"` antes de siquiera tocar el análisis de la clave:

```rust
// Todos estos devuelven Err(Error::Dkim(...)) en lugar de construir un firmante:
DkimSigner::from_pkcs8_pem("example.com\r\nX-Injected: evil", "default", pem);
DkimSigner::from_pkcs8_pem("example.com", "default\"", pem);
DkimSigner::from_pkcs8_pem("example.com;d=evil.com", "default", pem);
```

:::caution[Esto cerró un hueco real de inyección de cabeceras]
Todo otro valor de cabecera en la capa MIME de trustmailer se sanitiza a través de un
único punto central antes de escribirse en la conexión. `domain`/`selector` eran el único
camino que lo evitaba: se interpolan directamente en el valor de `DKIM-Signature` vía
`format!`, así que un CRLF sin validar en cualquiera de los dos habría permitido a quien
llamara inyectar cabeceras arbitrarias (o terminar la cabecera e inyectar contenido en el
cuerpo) en cada mensaje firmado con DKIM — alcanzable en la práctica desde
`dkim.domainName`/`dkim.keySelector` de `trustmailer-node`, es decir, directamente desde
configuración del lado de JS no del todo confiable. Se encontró de forma independiente en
dos rondas de auditoría distintas que miraban el crate DKIM y la fachada de Node desde
ángulos diferentes, y se corrigió de raíz: la validación ahora ocurre una sola vez, en el
constructor, en lugar de intentar hacer pasar el valor interpolado de vuelta por el
sanitizador general de cabeceras. `;` y `"` también se rechazan, ya que cualquiera de los
dos podría terminar la propia lista de etiquetas antes de tiempo dentro de la firma. Tanto
el crate de Rust como el binding de Node exponen el rechazo como un error real y tipado,
en lugar de un mensaje genérico de "clave inválida".
:::

## Plegado de línea

Las claves grandes (2048/4096 bits, los tamaños que cualquiera debería usar de verdad en
producción) producen un valor `b=` lo bastante largo como para empujar la cabecera
`DKIM-Signature` generada más allá del límite de 998 octetos por línea de RFC 5322.
trustmailer pliega la cabecera en los límites de etiquetas `;` — nunca en medio del valor
de una etiqueta — insertando `"\r\n\t"` en cuanto una línea crecería más allá de 76
columnas. Esto es seguro por construcción: la canonicalización relaxed de cabeceras
despliega (`CRLF` + espacio en blanco colapsa de vuelta a un solo espacio) antes de
hashear, tanto en el lado que firma como en el que verifica, así que plegar no cambia en
nada lo que realmente se firma o se verifica.

## Rendimiento: firmar no bloquea tu runtime asíncrono

Firmar con RSA-2048 toma unos pocos milisegundos, no microsegundos — barato en términos
absolutos, pero caro respecto a lo rápido que un ejecutor asíncrono espera que un
`poll()` regrese. `SigningTransport::send` ejecuta la llamada de firma real dentro de
`tokio::task::spawn_blocking` en objetivos nativos, para que no pueda estancar el hilo de
trabajo de Tokio compartido que esté sondeándolo y así privar de recursos a otros envíos
concurrentes. En `wasm32` no hay un pool de hilos bloqueantes al que delegar (y el
runtime es de un solo hilo de todos modos), así que ahí la firma se ejecuta en línea —
el mismo comportamiento, solo que sin la delegación, ya que no hay nada más que proteger.

## Contrastado contra la salida real de nodemailer

`trustmailer-dkim` incluye una prueba dorada que firma un mensaje fijo con una clave fija
y comprueba que el `bh=` y el `b=` resultantes coinciden, byte a byte, con la salida
capturada de forma independiente del propio firmante DKIM de nodemailer, dado la misma
clave y mensaje. RSA-SHA256 bajo PKCS#1v1.5 es determinista, así que esta es una
comprobación bastante más fuerte que "la salida parece una firma válida" — confirma que
trustmailer y nodemailer calculan literalmente la misma firma a partir de las mismas
entradas. El paso de canonicalización relaxed del cuerpo tiene su propia prueba dorada
separada, portada directamente del fixture de `relaxed-body-test.js` de nodemailer.

## Lo que deliberadamente no está aquí (todavía)

Algunas cosas que el soporte DKIM de nodemailer ofrece y que el código fuente de
trustmailer no implementa actualmente — anotadas aquí en lugar de dejarlas para que se
descubran por ensayo y error:

- **Sin rotación de claves / múltiples claves simultáneas.** nodemailer acepta un array
  `keys` para que un mensaje pueda firmarse con más de una clave (por ejemplo durante una
  migración de selector). `DkimSigner` guarda exactamente una clave; encadenar varios
  `SigningTransport` en secuencia no es equivalente, ya que solo el `DKIM-Signature` más
  externo terminaría cubriendo a los demás.
- **Sin elección de `hashAlgo`.** La firma es siempre RSA-SHA256; no hay opción para
  elegir un algoritmo de resumen distinto.
- **Sin caché en disco para mensajes enormes** (los `cacheDir`/`cacheTreshold` de
  nodemailer). trustmailer construye y hashea el mensaje canonicalizado en memoria.
- **Sin anulación de DKIM por mensaje.** La firma se configura una sola vez, al momento
  de `SigningTransport::new`/`createTransport({ dkim: {...} })`, para todos los mensajes
  que ese transporte envíe — no hay un campo `dkim` por llamada individual a
  `sendMail()`.
- **Aún no conectado a través de los bindings JS de navegador/WASM ni Cloudflare
  Workers.** A nivel de Rust, `SigningTransport<T>` es genérico sobre cualquier
  `Transport` y tiene una ruta de firma en línea dedicada (sin `spawn_blocking`)
  específicamente para `target_arch = "wasm32"`, así que el soporte para `wasm32`
  claramente se tuvo en mente. En la práctica, sin embargo, compilar de verdad
  `trustmailer-dkim` para `wasm32-unknown-unknown` tropieza hoy con un hueco de
  dependencias conocido y preexistente: la dependencia transitiva `rand` → `getrandom`
  del crate `rsa` necesita una funcionalidad de entropía para WASM habilitada que nada
  en el grafo de dependencias solicita actualmente. Ni `trustmailer-wasm` ni
  `trustmailer-smtp-cfworkers` dependen en absoluto de `trustmailer-dkim`/`rsa`, así que
  ese hueco no les afecta — pero ninguno de los dos expone hoy una opción `dkim` en su
  `createTransport()` como sí lo hace el binding de Node de todos modos. Solo
  `trustmailer-node` conecta DKIM por ahora.

Consulta [Seguridad](/es/docs/security/) para el panorama completo de lo auditado en el
resto del código, y la [matriz de runtimes](/es/docs/runtime-matrix/) para saber dónde la
firma DKIM ya es alcanzable (y dónde todavía no) según el runtime.
