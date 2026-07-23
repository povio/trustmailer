---
title: Direcciones
description: Cómo trustmailer interpreta, agrupa, renderiza y protege los campos de dirección — desde un único buzón hasta grupos RFC 5322 anidados.
---

Todo campo de dirección de un [Message](/es/docs/message/) — `from`, `to`, `cc`, `bcc`,
`reply_to` — se construye a partir del mismo tipo `Address` y el mismo parser, portado
del `addressparser` de nodemailer. Ese parser es deliberadamente **infalible**: las
bandejas de entrada reales están llenas de texto de dirección malformado, medio
entrecomillado y cargado de comentarios, y una librería de correo que lo rechaza sin
más es menos útil que una que se degrada con elegancia, igual que tiene que hacerlo el
propio campo "Para" de un cliente de correo. `parse_address_list` nunca devuelve un
error — en el peor caso, una entrada malformada se convierte en un `Address` con
`name` o `address` vacíos, en lugar de descartarse o entrar en pánico.

## La forma de `Address`

```rust
pub struct Address {
    pub name: String,
    pub address: String,
    pub group: Option<Vec<Address>>,
}
```

Un `Address` es o bien un único buzón (`group` es `None`) o un grupo RFC 5322 (`group`
es `Some(members)`, `address` está vacío, `name` es la etiqueta del grupo). Constrúyelo
directamente cuando ya tengas datos estructurados:

```rust
use trustmailer::address::Address;

let plano = Address::new("bob@example.com");
let con_nombre = Address::with_name("Bob Smith", "bob@example.com");
let grupo = Address::group("Equipo de ventas", vec![
    Address::new("alice@example.com"),
    Address::new("carol@example.com"),
]);
```

## Formatos de texto aceptados

`to`, `cc`, `bcc` y `reply_to` aceptan cualquier tipo que implemente `IntoAddresses`, lo
que cubre un `&str`/`String` (interpretado como lista de direcciones), un único
`Address`, un `Vec<Address>`, o un `Vec<&str>` (cada string se interpreta por separado y
se concatena). Como texto plano, el parser reconoce las mismas formas que nodemailer:

| Entrada | Se interpreta como |
|---|---|
| `"bob@example.com"` | `Address { name: "", address: "bob@example.com" }` |
| `"Bob Smith <bob@example.com>"` | `Address { name: "Bob Smith", address: "bob@example.com" }` |
| `"Bob Smith bob@example.com"` (sin ángulos) | Igual que arriba — se detecta una dirección dentro del texto libre incluso sin `<...>` |
| `"bob@example.com (Bob Smith)"` | Igual que arriba — un comentario entre paréntesis al final se usa como nombre visible cuando no hay otro nombre presente |
| `"\"Smith, Bob\" <bob@example.com>"` | `Address { name: "Smith, Bob", address: "bob@example.com" }` — un nombre visible entrecomillado puede contener una coma o punto y coma sin que se lea como separador de campo |
| `"bob@example.com, carol@example.com"` o `"bob@example.com; carol@example.com"` | Dos entradas — tanto `,` como `;` se aceptan como separadores de nivel superior |

```rust
use trustmailer::Message;

let message = Message::builder()
    .from("Alice <alice@example.com>")
    .to("Bob Smith <bob@example.com>, \"Carol, PhD\" <carol@example.com>")
    .build();
```

:::tip[Entrecomilla los nombres que contengan una coma]
`"Joe Foo, PhD <joe@example.com>"` (sin comillas) igualmente se recupera bien — el
parser recombina una serie de fragmentos que son solo nombre seguidos de uno con nombre
y dirección a la vez — pero esto es una heurística de mejor esfuerzo, no una garantía
para cualquier caso límite. Envolver el nombre visible entre comillas dobles,
`"\"Joe Foo, PhD\" <joe@example.com>"`, es la forma fiable de incluir una coma, y es lo
que el propio trustmailer siempre emite al renderizar un nombre de vuelta (ver
[Renderizar de vuelta a una cabecera](#renderizar-de-vuelta-a-una-cabecera)).
:::

## Grupos de direcciones (RFC 5322)

Un grupo nombra una lista de destinatarios como una unidad —
`Nombre-Grupo: a@x.com, b@y.com;` — y se interpreta como un `Address` con
`group: Some(...)`:

```rust
use trustmailer::address::parse_address_list;

let parsed = parse_address_list("Ventas:alice@example.com, carol@example.com;");
assert!(parsed[0].is_group());
```

Un grupo vacío (`"Sin-revelar:;"`) se interpreta como un grupo con cero miembros — el
equivalente en trustmailer del idioma de destinatarios no revelados de nodemailer. Los
grupos pueden anidarse (`"Outer:Inner:deep@example.com;;"`), y
`parse_address_list_flatten` (el `{ flatten: true }` de nodemailer) expande cada grupo
a sus buzones hoja, descartando por completo la estructura de grupo:

```rust
use trustmailer::address::parse_address_list_flatten;

let plano = parse_address_list_flatten("Group1:a@b.com, Group2:c@d.com;;");
// -> [Address { address: "a@b.com", .. }, Address { address: "c@d.com", .. }]
```

El envelope SMTP no tiene concepto de grupo — `Message::effective_envelope()` siempre
aplana `to`/`cc`/`bcc` antes de derivar `RCPT TO`, así que un grupo en cualquiera de
esos campos sigue llegando a todos sus miembros. Ver [Mensaje](/es/docs/message/#envelope-y-effective_envelope)
para la derivación completa del envelope.

## Límites y protecciones

Como los campos de dirección son entrada potencialmente hostil (una cabecera
`To:`/`Cc:` de correo entrante, o un valor reenviado desde un origen no confiable), el
parser acota su propio trabajo en lugar de confiar en que la entrada esté bien formada
o acotada en tamaño:

:::caution[Dos topes independientes]
- **`MAX_ADDRESS_LIST_ENTRIES` (4096)** — una cadena de lista de direcciones solo se
  interpreta hasta esta cantidad de entradas de nivel superior (separadas por coma o
  punto y coma); lo que exceda el tope se trunca en lugar de interpretarse por
  completo, acotando el CPU/memoria ante un valor `To`/`Cc` malintencionadamente enorme.
- **`MAX_NESTED_GROUP_DEPTH` (50)** — RFC 5322 no permite grupos anidados en absoluto,
  así que cualquier anidamiento más profundo que esto se trunca. Este tope se aplica en
  dos lugares independientes: en el propio parser de texto, *y* en
  `Address::flatten`/`Address::to_header_value` — esto último protege ante un grafo de
  `Address` construido directamente (no interpretado por el parser), por ejemplo uno
  deserializado desde JSON no confiable a través del soporte `serde` de `Address`, que
  de otro modo podría recursionar sin límite y arriesgar un desbordamiento de pila.

Ambos topes truncan silenciosamente en lugar de devolver un error, coherente con el
contrato general del parser de ser infalible por diseño. Ver
[Seguridad](/es/docs/security/) para el resto de las protecciones portadas desde los
propios tests de regresión de nodemailer, incluida la corrección de inyección de
cabeceras que estos topes complementan.
:::

Algunos detalles de parseo más específicos que vale la pena conocer:

- **Literales de dominio** — `user@[IPv6:2001:db8::1]` conserva los dos puntos dentro
  del corchete `[...]` en lugar de leerlos incorrectamente como separador de grupo; un
  `[` sin cerrar no se traga el resto de la lista
  (`"alice@x.com, bob[@x.com, carol@x.com"` sigue produciendo tres direcciones
  separadas).
- **Normalización de finales de línea** — un `\r` suelto (que no forma parte de un par
  `\r\n`) dentro del texto de una dirección se normaliza a un espacio, exactamente igual
  que un `\n` suelto, en lugar de descartarse silenciosamente.
- **Partes locales entrecomilladas** — `"user@attacker.com"@legitimate.com` se
  interpreta como una única dirección cuya parte local resulta contener una `@`, y no se
  lee erróneamente como `user@attacker.com` con un dominio final suelto; el parser nunca
  deja que una parte local entrecomillada engañe a la heurística de "detectar una
  dirección suelta dentro de texto libre" para que elija la `@` equivocada.

## Renderizar de vuelta a una cabecera

`Address::to_header_value()` (usado internamente cada vez que se escribe una cabecera
`From`/`To`/`Cc`/`Reply-To`) renderiza un buzón como `Nombre <dirección>` o un grupo
como `Nombre: miembro, miembro;`, aplicando las reglas de RFC 5322/2047 al salir:

- Un nombre visible no ASCII se envuelve como una encoded-word RFC 2047:
  `=?UTF-8?B?<base64>?=`.
- Un nombre visible ASCII que contenga `,`, `;`, `<`, `>` o `"` se entrecomilla, con
  cualquier `\` o `"` literal dentro escapado (primero `\`, luego `"` — escapar en el
  orden contrario duplicaría el escape de las barras invertidas recién insertadas y
  podría dejar la cadena entrecomillada sin cerrar).
- Un nombre ASCII plano sin ninguno de esos caracteres se emite tal cual.

Esta es también la ruta por la que pasa cualquier valor de cabecera personalizado o de
`list` antes de escribirse en el cable — ver [Mensaje → Opciones de cabecera](/es/docs/message/#opciones-de-cabecera)
para las protecciones compartidas contra inyección de CRLF aplicadas ahí.

## Desde Node.js

`MailOptions.from`/`to`/`cc`/`bcc`/`replyTo` son strings simples, interpretados con
exactamente las mismas reglas descritas arriba — `to: "bob@example.com,carol@example.com"`
se comporta idéntico a la llamada en Rust
`.to("bob@example.com,carol@example.com")`, ya que el binding de napi reenvía el string
directamente al mismo método del builder:

```js
const info = await transporter.sendMail({
  from: "Alice <alice@example.com>",
  to: "Bob Smith <bob@example.com>, \"Carol, PhD\" <carol@example.com>",
  subject: "Hola",
  text: "¡Hola!",
});
```

:::note[Todavía no hay un objeto de dirección estructurado en el binding de Node]
El `MailOptions` actual solo acepta los campos de dirección como strings — todavía no
hay una forma de objeto `{ name, address }` expuesta desde Node (eso es un struct
`Address` plano de Rust en el lado del core). Si necesitas construir un nombre visible
de forma programática desde Node, compón tú mismo el string (entrecomillándolo si
contiene una coma) en lugar de depender de un literal de objeto.
:::

Ver [Mensaje](/es/docs/message/) para el resto del modelo de mensaje, y
[Adjuntos](/es/docs/attachments/) para cómo el `cid` de un adjunto (un identificador
relacionado pero distinto, no una dirección de correo) permite que el HTML referencie
una imagen incrustada.
