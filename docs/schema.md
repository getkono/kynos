# Schemas

## The rule

A type describes itself. Nothing outside a type may say what a value *is*.

`format` is an identity claim — `uuid`, `date-time`, `decimal` each assert that a
string is a particular kind of thing. That claim follows from the Rust type or it
follows from nothing, so it belongs to the type's [`Schema`](../crates/kynos/src/schema/mod.rs)
implementation and to no other place. Annotating a `String` as a UUID is the
anti-pattern this document exists to close: it puts the claim somewhere the
compiler cannot check it and the type cannot honour it.

Field constraints are the opposite case and stay where they are. `minimum`,
`pattern`, `min_length` and their siblings are business rules about a particular
field, not statements about what the type is, so they are declared per field with
`#[schema(...)]`.

**Policy:**

- `format` is not part of the `#[schema(...)]` field grammar. Naming it is a
  compile error that names the remedy.
- A vendor or application format is expressed by a type — either one of the
  feature-gated types below, or a newtype with its own `Schema` implementation.
- A type whose emitted `format` does not match what serde actually reads and
  writes is a bug, not a convenience. The description follows the wire form.

## Where `format` values come from

Three vocabularies, and the distinction matters because their guarantees differ.

| Source | Members | Guarantee |
| --- | --- | --- |
| Defined by OAS itself | `int32`, `int64`, `float`, `double`, `password` | Named in the specification ([3.1.2 §Data Type Format](../references/3.1.2.md)) |
| The JSON Schema Validation vocabulary | `date-time`, `date`, `time`, `duration`, `uuid`, `uri`, `ipv4`, `ipv6`, `email`, `regex` and the rest of §7.3 | Non-validating annotations by default |
| The [OAI Format Registry](https://spec.openapis.org/registry/format/) | `date-time-local`, `time-local`, `decimal`, `decimal128`, `char`, `int8`–`int64`, `uint8`–`uint64`, `http-date`, `media-range`, … | "Support for any registered format is strictly OPTIONAL, and support for one registered format does not imply support for any others" |

Two consequences Kynos relies on:

- **Unrecognised formats degrade, they do not break.** "Tools that do not
  recognize a specific `format` MAY default back to the `type` alone, as if the
  `format` is not specified." So a registered-but-obscure format costs nothing,
  and emitting bounds *alongside* a width format means a tool that ignores
  `uint32` still receives the real constraint.
- **An unregistered format is legal.** The vocabulary is open. Kynos emits one
  exactly once, for `jiff::Zoned`, and says so below.

`binary`, `byte` and `base64url` are registered but **deprecated**. Kynos never
emits them; see [Binary content](#binary-content).

## The standard library

Every row below is built. A leaf implementation returns its schema directly; a
composite — `Vec<T>`, a map, a tuple, a derived struct — reaches its members
through [`Registry::resolve`](../crates/kynos/src/schema/registry.rs), which
registers a named type once and hands back a `$ref`.

| Rust | `type` | `format` | Also emitted |
| --- | --- | --- | --- |
| `bool` | `boolean` | — | |
| `String` | `string` | — | |
| `char` | `string` | `char` | `minLength: 1`, `maxLength: 1` |
| `i8`, `i16`, `i32` | `integer` | `int8`, `int16`, `int32` | the type's exact range |
| `u8`, `u16`, `u32` | `integer` | `uint8`, `uint16`, `uint32` | the type's exact range |
| `i64` | `integer` | `int64` | — |
| `u64` | `integer` | `uint64` | `minimum: 0` |
| `f32`, `f64` | `number` | `float`, `double` | |
| `Option<T>` | `T`, widened to admit `null` | | |
| `Box<T>`, `Arc<T>` | `T`, under `T`'s own component name | | |
| `Vec<T>`, `VecDeque<T>`, `[T]` | `array` | — | `items` |
| `[T; N]` | `array` | — | `minItems: N`, `maxItems: N` |
| `HashSet<T>`, `BTreeSet<T>` | `array` | — | `uniqueItems` |
| `HashMap<K, V>`, `BTreeMap<K, V>` | `object` | — | `K: MapKey` supplies `propertyNames` |
| tuples up to twelve | `array` | — | `prefixItems`, closed with `items: false` and bounded by `minItems` |
| `()` | `null` | — | |
| `Ipv4Addr`, `Ipv6Addr` | `string` | `ipv4`, `ipv6` | |
| `IpAddr` | — | — | `anyOf` of the two above |

`i64` and `u64` carry no maximum because `i64::MAX` and `u64::MAX` are not
representable in an `f64`, and JSON Schema bounds are numbers. A rounded bound
would forbid values the type accepts or accept values it does not, so the width
is left to the format, which is the only honest thing the vocabulary can say.
`u64` keeps `minimum: 0` because that bound *is* exactly representable.

The unsigned widths are registered, so a `u32` is `uint32` and not a widened
`int64`. Earlier revisions widened because only the signed OAS formats were
known; that workaround is gone.

## Behind a feature flag

A scalar type from outside `std` gets an implementation only when its crate is a
Kynos dependency, and each such crate arrives feature-gated and additive. The
umbrella flags exist so the shape of a concept is defined once, not once per
backend; enabling an umbrella without a backend is a compile error.

| Feature | Requires | Adds |
| --- | --- | --- |
| `uuid` | — | `uuid` |
| `time` | one of the two below | *(shapes only)* |
| `time-chrono` | `time` | `chrono` |
| `time-jiff` | `time` | `jiff` |
| `decimal` | one of the two below | *(shapes only)* |
| `decimal-rust` | `decimal` | `rust_decimal` |
| `decimal-big` | `decimal` | `bigdecimal` |

Every feature above is built.

| Rust | `type` | `format` | Feature |
| --- | --- | --- | --- |
| `uuid::Uuid` | `string` | `uuid` | `uuid` |
| `chrono::NaiveDate` | `string` | `date` | `time-chrono` |
| `chrono::NaiveTime` | `string` | `time-local` | `time-chrono` |
| `chrono::NaiveDateTime` | `string` | `date-time-local` | `time-chrono` |
| `chrono::DateTime<Utc>`, `<FixedOffset>` | `string` | `date-time` | `time-chrono` |
| `jiff::civil::Date` | `string` | `date` | `time-jiff` |
| `jiff::civil::Time` | `string` | `time-local` | `time-jiff` |
| `jiff::civil::DateTime` | `string` | `date-time-local` | `time-jiff` |
| `jiff::Timestamp` | `string` | `date-time` | `time-jiff` |
| `jiff::Zoned` | `string` | `date-time-zoned` + `pattern` | `time-jiff` |
| `jiff::Span`, `jiff::SignedDuration` | `string` | `duration` | `time-jiff` |
| `rust_decimal::Decimal` | `string` | `decimal` | `decimal-rust` |
| `bigdecimal::BigDecimal` | `string` | `decimal` | `decimal-big` |

### Why the offset-less types are not `date-time`

RFC 3339's `date-time` and `full-time` both **require** a UTC offset, and
`chrono::NaiveDateTime` and `jiff::civil::DateTime` carry none. The registry's
`date-time-local` and `time-local` exist for exactly this case: "RFC 3339
date-time without the timezone component".

Claiming `date-time` for them would break both halves of the exchange, and the
request half is the serious one. `chrono::NaiveDateTime`'s `Deserialize` parses
through `FromStr`, which **rejects** a trailing `Z` or a numeric offset. A
description claiming `date-time` invites a consumer to send
`2026-03-15T14:00:00Z`, which the service then answers 400 for — a documented
input that cannot work.

Kynos cannot inject an offset on the type's behalf. `Schema` describes; it does
not serialize, and owning the serde implementation for a foreign type is not
available. Nor is `#[serde(with = ...)]` an answer: the derive rejects it,
because it decouples the wire form from the Rust type and so from the schema.

An offset-less type is not a deficient instant. It models civil time — an opening
hour, a birthday — which genuinely has no offset. The type that models an instant
is `DateTime<Utc>` or `jiff::Timestamp`, and those need none of this.

### `jiff::Zoned` and the one unregistered format

`Zoned` serializes as RFC 9557 — `2024-06-19T15:22:00-04:00[America/New_York]`.
The bracketed IANA annotation makes the string a superset of RFC 3339 and
therefore not a valid `date-time`. The registry has no RFC 9557 entry.

It is emitted as `format: date-time-zoned` with a `pattern` constraining the
shape. The format name is ours until it is registered, which is a request worth
filing; until then a consumer that does not know it falls back to a constrained
string, which is correct rather than merely tolerable.

### Decimals are strings

Both backends serialize a decimal as a JSON **string** by default, and the schema
follows. This is not incidental: a JSON number round-trips through an `f64` in
most consumers, losing exactly the precision a decimal exists to keep. The
registry allows `decimal` on `string` or `number`; Kynos emits the string.

`rust_decimal`'s `serde-float` feature flips serialization to a number, and Cargo
feature unification means any crate in the graph enabling it flips it for
everyone — at which point the emitted `type: string` is wrong and nothing in the
type system notices. A unit test asserts the serialized form is a JSON string, so
that unification fails the build rather than silently invalidating descriptions.

`decimal128` means IEEE 754-2008 decimal128, which neither backend is. It is
reserved for a backend that implements it and is emitted by nothing today.

## Binary content

Binary is fully in scope. What Kynos never emits is the OAS 3.0 spelling: "the
`format` keyword has no effect on the content-encoding of the schema in OAS 3.1.
Instead, JSON Schema's `contentEncoding` and `contentMediaType` keywords are
used."

`Binary<M>` emits one of three shapes, and which one applies is decided by
where the bytes sit rather than by the type.

| Case | Emitted |
| --- | --- |
| A raw binary message body | **no `type` at all** — raw binary is outside `type` |
| A raw binary body whose media type is already the Media Type Object key | the empty schema; `contentMediaType` would be redundant |
| Binary embedded in a text format — a JSON field, a form value | `type: string` with `contentEncoding: base64`, or `base64url` in a query string or `application/x-www-form-urlencoded` body, which avoids re-encoding |

Two further rules bind:

- `contentMediaType` **shall be ignored** where it contradicts a relevant Media
  Type or Encoding Object, so it is never emitted contradicting one.
- `maxLength` may bound a streaming payload — counted in octets for unencoded
  binary, in characters for encoded. This is where a body-size limit becomes
  visible in a schema.

`contentEncoding` is unrelated to HTTP's `Content-Encoding`, which is about
compression and is applied after all of this.

## Deliberately unmapped, and the remedy

A type that cannot produce a *constraining* schema has no implementation. There
is no degradation to `{}` behind your back — see
[`Unchecked`](../crates/kynos/src/schema/unchecked.rs) for saying so on purpose.

| Rejected | Why | Use instead |
| --- | --- | --- |
| `serde_json::Value`, `Map`, `RawValue` | the schema would be `true` | a derived type, or `Unchecked` |
| `HashMap<String, Value>` | `additionalProperties: true` | `HashMap<String, T> where T: Schema` |
| `usize`, `isize` | width depends on the build target; a wire contract must not | `u32`/`u64`/`i32`/`i64` |
| `u128`, `i128` | outside JSON's safe integer range, and no registered format covers them | a newtype over `String` carrying its own `Schema`, or `u64` |
| `std::time::SystemTime`, `Instant`, `Duration` | serde emits a seconds/nanos pair nobody wants as a contract | `chrono::DateTime<Utc>` or `jiff::Timestamp`; `jiff::Span` for a duration |
| `chrono::TimeDelta` | serializes as a `[seconds, nanos]` array, the shape `std::time::Duration` is refused for | `jiff::Span`, or a newtype emitting `string`/`duration` |
| `chrono::DateTime<Local>` | the zone depends on the process environment, which is the `usize` argument in another guise | `DateTime<Utc>`, or `DateTime<FixedOffset>` to keep an offset |
| `PathBuf`, `OsString` | platform-dependent, not guaranteed to be UTF-8 | `String` |
| `Box<dyn Trait>` | no schema exists | a closed enum deriving `Schema` |

Each row has a `compile_fail` case under
[`tests/ui/schema/`](../crates/kynos/tests/ui/schema/) and a passing sibling that
differs in exactly the property under test; the count is asserted, so a row added
without a case fails the build.

## `Schema` is not a serde bound

`Schema` names no serde trait, and it should not. The two answer different
questions and are bounded separately at every use site:

| Half | Bound | Why |
| --- | --- | --- |
| what goes on the wire | `Serialize` / `DeserializeOwned` | `Json<T>: FromRequest`, `Json<T>: IntoResponse` |
| what the document claims | `Schema` | `Json<T>: Describe`, `Json<T>: Responses` |

`#[derive(Schema, Serialize, Deserialize)]` is therefore three answers to three
questions, not one repeated three times. It is common because most described
types are JSON bodies that genuinely round-trip both ways — not because the
derives are redundant.

Coupling them would be a real narrowing, and the examples already show what it
would break:

- `protobuf.rs` derives `prost::Message` and `Schema` with no serde at all. A
  described type is not always a serde type.
- `payloads.rs` derives `Schema` and `MultipartForm` for a multipart body, and
  no serde at all: a multipart payload is decoded part by part rather than
  through a `Deserializer`, so what carries it is `FromPart`/`IntoPart`.
- A response-only type would be forced to implement `Deserialize`, and a
  request-only type `Serialize`, each to satisfy a direction it never travels.

There is also a versioning cost: a serde supertrait would put a specific serde
major version in a framework trait bound, so a program could not describe a type
whose serde differed from the framework's.

The two are conjoined at exactly one place —
[`Representation`](../crates/kynos/src/response/negotiate/representation.rs),
where a negotiated alternative must both serialize and describe itself — and
even there the conjunction is written at the impl site rather than pushed into
the trait.

## Unions

A Rust enum is described as `oneOf`. Which shape the branches take follows
serde's tagging, because the description has to match what serde actually reads
and writes:

| serde | Emitted |
| --- | --- |
| every variant is a unit | `type: string` with an `enum` of the names, unless one is deprecated — see [Deprecation](#deprecation) |
| externally tagged, the default | `oneOf`; a unit variant is its own name as a `const` string, and anything else a one-property object keyed by the variant name and closed with `additionalProperties: false`, since serde reads it as exactly one entry. A variant's `alias` widens each name it carries — see [Aliases](#on-a-variant) |
| `#[serde(tag = "...")]` | `oneOf` of objects each carrying the tag as a `const` property beside the variant's own, plus a `discriminator`. A newtype variant has no properties to sit beside, so it becomes an `allOf` of a tag-only object and its payload — which must implement `Flatten`, for the reason a [flattened](#flattening) field's type must. A map payload has no route through `#[schema(open)]`, since serde refuses `flatten` on a newtype variant: write a struct variant holding the map as a flattened open field, which serializes the same way |
| `#[serde(tag = "...", content = "...")]` | the same, with the payload under the content property, which a unit variant omits |
| `#[serde(untagged)]` | **refused**, on the enum and on any variant serde reads or writes: an untagged variant goes on the wire as its bare payload, so a branch keyed by its name describes a value serde never writes. On a variant serde skips both ways it is in no schema, and is accepted |
| a `#[serde(other)]` variant serde reads | **refused**: it accepts every tag the enum does not name, and only 3.2's `defaultMapping`, which the derive does not emit, could say so. On a variant serde skips both ways it catches nothing, and is accepted |

`discriminator` is emitted exactly when a tag is present, because that is when
there is a property every branch carries for a consumer to switch on.

### Why untagged is refused

`oneOf` without a discriminator is not a decoding rule. It says a payload
matches one of these shapes; it does not say *which*, and serde's answer —
first branch that deserializes, in declaration order — is not expressible in
JSON Schema. A generator reading it has to guess, and two generators may guess
differently while both honouring the description. That is a worse failure than
refusing, because it is silent.

So the refusal stands, and it is a claim about what is describable rather than
an unimplemented case.

**This was reopened and re-answered.** The question was whether a downstream
consumer needed untagged types; the acceptance contract it came from asks for
*"enums, tagged unions, `oneOf`"* and never names untagged. Everything on that
list is already emitted, so nothing was blocked. Reading the contract rather
than the summary of it is what settled the question.

### What to write instead

Add a tag. `#[serde(tag = "kind")]` costs one property on the wire and buys a
`discriminator`, which is the construct that makes the choice determinable
rather than guessable.

Where the payload genuinely is arbitrary — a proxy passthrough, a third party's
webhook envelope — that is what
[`Unchecked<T>`](../crates/kynos/src/schema/unchecked.rs) is for: it emits the
permissive schema with an annotation saying the shape is unspecified on
purpose, which is honest where an ambiguous `oneOf` is not.

`#[serde(untagged)]` on anything that is not an enum is serde's to refuse, and
it does. The derive stays quiet there rather than adding a second diagnostic
about enums to a struct.

## Deprecation

`#[deprecated]` — Rust's own attribute, not a Kynos one — becomes
`deprecated: true` wherever a description can carry it: on the type, on a field,
on an enum variant, and on a handler, where it marks the operation.

There is deliberately no `#[schema(deprecated)]` key. A second spelling would
let the two disagree, and the disagreement has a bad direction: a field marked
in the description but not in the compiler is a deprecation the people most able
to act on it are never warned about. Reading the language's attribute keeps one
fact in one place.

The `note` is not read. `#[deprecated(note = "...")]` addresses a Rust caller at
the call site, and `deprecated` in a description is a boolean; forwarding the
note would repeat advice about a Rust API to a consumer that has none.

`deprecated: false` is never emitted. The keyword defaults to false, so writing
it out states nothing and puts a word in every schema in the document.

### The one shape that has to change

An enum whose variants are all units is normally `type: string` with an `enum`
array of the names. That array is *one schema shared by every name*, so it has
nowhere to record that one of them is retired.

Deprecating a unit variant therefore drops the compact shape for the `oneOf` of
`const` branches, which describes exactly the same wire values and gives each
name a schema of its own to mark:

```json
{ "oneOf": [
    { "type": "string", "const": "Web" },
    { "type": "string", "const": "Fax", "deprecated": true }
] }
```

An enum with no deprecated variant is untouched. The alternative was emitting
nothing and leaving the description disagreeing with the type it came from,
which is the failure this codebase treats as worse than a verbose shape: nobody
can see it.

## Flattening

`#[serde(flatten)]` makes a field's members the *parent's* members, so the
parent cannot name them — it composes the field's schema into its own `allOf`
instead. That composition is only correct when the flattened schema names every
member it constrains, and `allOf` is why: `additionalProperties` is defined
against the `properties` and `patternProperties` of **its own** schema object,
and a branch of an `allOf` has neither, so the keyword reaches every member of
the instance including the ones the parent declared itself.

A map is the shape that hits it. It has no fixed member names to put in
`properties`, so its whole description *is* `additionalProperties` — and
flattening one used to emit an object requiring `id: u64` to be a string.

**So a flattened field's type must implement
[`Flatten`](https://docs.rs/kynos/latest/kynos/schema/flatten/trait.Flatten.html).**
The `Schema` derive asserts the bound once per flattened field, in a `const _`
witness spanned at the field's type, so the refusal lands where it was written.
The derive implements the marker for the shapes whose description is an object
naming its members: a struct with named fields, and an enum whose every `oneOf`
branch is such an object. It does not for a container carrying `#[schema(open)]`
or `#[serde(transparent)]`, an externally tagged enum, whose object branches
are [closed](#closed-objects) to all but the variant key, or an internally
tagged enum with a newtype variant, nor for a struct or an internally
tagged struct variant serde writes that holds a named field serde never reads,
whose schema leaves out a member serde writes. Nor does it for a shape that
[`deny_unknown_fields`](#closed-objects) closes, and a closed object also
bounds its flattened fields by the narrower `ClosedFlatten` described there. A
variant serde skips both ways counts toward none of these, and one serde reads
and never writes counts as any other. `Box<T>` and `Arc<T>` carry `T`'s answer
across,
`Problem` implements it so a problem document can carry extension members of
its own type, and the trait is unsealed so a hand-written `Schema` doing the
same can say so. The rule a flattened schema has to meet is that it marks every
member it contributes as evaluated, and constrains none it does not name.
`Problem`'s `additionalProperties: true` meets both from inside the `allOf`: it
reaches the parent's members and permits them. The five members it names it
does constrain, so a parent member may not reuse `type`, `title`, `status`,
`detail` or `instance`. A map's `additionalProperties` refuses the
parent's members, and `Unchecked`'s permissive schema marks none of its own
evaluated, so neither is flattenable.
serde offers nothing to read here: `flatten` never leaves `serde_derive` and
what enforces it is a runtime serializer, so the type-level surface has to be
Kynos's own.

### `#[schema(open)]`, when the object really is open

A flattened map is a real shape, and refusing it outright would remove it with
no way back. `#[schema(open)]` on the flattened field is the declaration that
the object admits members nothing names. It swaps the `Flatten` bound for
[`OpenMap`](https://docs.rs/kynos/latest/kynos/schema/flatten/trait.OpenMap.html)
and changes what is emitted: the flattened schema's `additionalProperties` is
hoisted onto the parent as `unevaluatedProperties`.

The hoist needs the map's own schema object in hand, which is what `OpenMap`
asks for. `HashMap` and `BTreeMap` implement it, as does a `Box` or `Arc` of
one. A named type — a newtype over a map, or a struct with an open field of its
own — resolves to a `$ref`: there is nothing to hoist, and the
`additionalProperties` of the schema it refers to would reach the parent's
properties from inside the `allOf`. So it is a compile error at the field.

```rust,ignore
#[derive(Schema, Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
}
```

```json
{ "type": "object",
  "properties": { "id": { "type": "integer", "format": "uint64", "minimum": 0 } },
  "required": ["id"],
  "allOf": [{ "type": "object" }],
  "unevaluatedProperties": { "type": "string" } }
```

`unevaluatedProperties` rather than `additionalProperties` because it is the one
keyword that sees `properties` annotations *across* an `allOf`. Hoisting to
`additionalProperties` would be correct for a lone flattened map and wrong the
moment a second flattened field contributed properties through a `$ref`.

The attribute may appear once per object — there is one
`unevaluatedProperties` to supply — and only on a flattened field, since an
ordinary field is a single property whose own schema already states what it
admits. Both are compile errors.

`Unchecked` over a map — a `serde_json::Map`, or a type that implements `OpenMap`
itself — implements `OpenMap` too, and is the route for arbitrary JSON beside an
object's own members. Its schema is written in place with no
`additionalProperties`, so the hoist moves nothing and the object stays open.
A payload that is not a map, such as `Unchecked<u64>`, is refused: serde
flattens only structs and maps, and a struct is flattened as itself.
It does not implement `Flatten`: the permissive schema names none of the
members it contributes, so beside an open map they stay unevaluated and the
map's `unevaluatedProperties` refuses what serde writes.

Hoisting nothing is also why a named field serde writes and never reads,
`skip_deserializing` alone, may sit beside an open `Unchecked` and not beside
an open map of typed values. The schema leaves that field out, and only a
hoisted `unevaluatedProperties` that constrains something would refuse it. The
derive bounds an open field beside such a field by
[`AdmitsAny`](https://docs.rs/kynos/latest/kynos/schema/flatten/trait.AdmitsAny.html),
because whether the field's type hoists a constraint is visible to the
compiler and not to the derive. `Unchecked` implements it, and so does a
`HashMap` or `BTreeMap` whose values are `Unchecked`: its hoisted value schema
is the permissive one and refuses nothing. A map whose values are typed does
not, and neither does a named type, which is not an open map at all.

One thing is lost deliberately: a map whose key type constrains
`propertyNames` contributes no key constraint through an open flatten. Inside
the `allOf` branch `propertyNames` names the parent's own properties too, which
is the defect above in its other form, and `patternProperties` — which could
express it — is not emitted. The key constraint is dropped rather than moved, so
the description stays weaker than the type instead of contradicting it. The
`allOf` keeps what is left of the map's schema, `{ "type": "object" }`, which
asserts nothing the parent does not.

A flattened `Problem`, or any flattened type that carries one, beside an open
map leaves the map's values unchecked. `Problem`'s `additionalProperties: true`
reaches through the `$ref` of a derived type that flattens it and marks every
member evaluated, the map's included, so the hoisted `unevaluatedProperties`
has nothing left to constrain. The description admits more than the type
writes and never refuses what it writes. Refusing the pair would take a second
marker, one every derived type would carry and `Problem` would not, since the
derive sees a field's syntax rather than which type it holds. In an object
[`deny_unknown_fields`](#closed-objects) closes, the same keyword would leave
the closing `unevaluatedProperties: false` nothing to refuse, and there such a
marker exists: `Problem` does not implement `ClosedFlatten`, and neither does a
derived type that flattens it, so that object is refused.

## Closed objects

serde refuses a key that names no field it reads when the type carries
`#[serde(deny_unknown_fields)]`, so the derive closes each object that rule
reaches. That covers a struct, each struct variant's fields under every tagging,
and an adjacently tagged branch. An internally tagged unit or newtype variant
stays open, because serde ignores the keys beside its tag or hands them to the
payload.

An externally tagged branch that is an object is closed with or without the
attribute, because serde reads it as exactly one entry named after the variant
and refuses anything beside it. So an externally tagged enum never implements
`Flatten`, even though serde can flatten one by taking the entry named after a
variant: the enum has one schema, and that closed branch inside an outer
object's `allOf` would refuse the members the object declared itself. Tag the
enum, internally or adjacently, to flatten it.

An object that composes nothing is closed with `additionalProperties: false`,
which every consumer reads. An object that composes a flattened struct through
an `allOf` is closed with `unevaluatedProperties: false`. That keyword sees the
struct's `properties` across the `allOf` and its `$ref`, whereas
`additionalProperties` would refuse them. A closed shape does not implement
`Flatten`, for the reason an open container does not: its closing keyword would
sit inside the outer object's `allOf` and refuse the members that object
declared itself. serde's documentation lists the attribute as unsupported with
`flatten` for the same reason.

A flattened field of a closed object is held to a narrower bound than
`Flatten`. serde buffers the object's entries, hands each flattened field all
of them, and then refuses the first entry no field took, and only a type serde
reads through `deserialize_struct` takes one. An internally tagged enum reads
through `deserialize_any`, and a struct holding a flattened field serde reads
through `deserialize_map`; both borrow the entries and leave every one behind,
so serde refuses each document the type writes while the closed schema
accepts it. The derive therefore bounds each flattened field of a closed object
by
[`ClosedFlatten`](https://docs.rs/kynos/latest/kynos/schema/flatten/trait.ClosedFlatten.html),
which it implements beside `Flatten` for two shapes. One is a struct with no
flattened field serde reads, by serde's own test, so one it skips both ways
does not count and a flattened `PhantomData` does, and with no container
`#[serde(tag = "...")]`, which serde writes beside the fields and never takes
([#208](https://github.com/getkono/kynos/issues/208) tracks the struct's own
schema leaving that tag out). The other is an adjacently tagged enum, whose tag
and content keys serde names. An externally tagged enum, which serde would read
by its variant key, is not `Flatten` at all, so a closed object flattening one
reports both refusals, and the `ClosedFlatten` one says only that the type is
not known to be read by name. `Box<T>` and `Arc<T>` carry the
answer across, `Problem`, which serde never reads, does not implement it, and a
hand-written `Flatten` has to implement it too to be flattened into a closed
object. The field stays bounded by `Flatten` as well, so a type that is not
flattenable at all is refused with that reason too. An internally tagged
newtype variant's payload is bounded by `Flatten` alone, since its tag-only
object is never closed.

A closed object has no true schema in two cases, so the derive refuses each
one:

- A flattened `#[schema(open)]` map. serde refuses every unknown key before the
  map sees it, so it reads the map empty and writes members it would refuse.
- A named field serde writes and never reads, `skip_deserializing` alone. It is
  left out of the schema, and the closed object refuses what serde writes of it.

An `alias` is not among them, because the object names every alias
([Aliases](#aliases)).

## Aliases

serde reads a named field under its wire name or under any
`#[serde(alias = "...")]` it carries, and refuses a document naming two of them
as a duplicate field. So each name is a property under the field's schema, and
one `allOf` entry per aliased field bounds how many of its names appear:

- A required field is present under exactly one name: a `oneOf` of one
  `required` per name. It is not listed in the object's own `required`.
- An optional field is present under at most one: a `not` over a `required`
  naming both of each pair of names, inside an `anyOf` when there are several
  pairs.

An alias is the literal name serde reads, since `rename_all` does not reach it,
and one repeating a name already read adds nothing. A flattened struct's aliases
reach the object it is flattened into through its `$ref`, as its other
properties do. The `allOf` makes a closed object's closing keyword
`unevaluatedProperties`, which sees the object's own `properties` as
`additionalProperties` would.

The default [`QueryParams::parameters`](../crates/kynos/src/extract/params/query.rs)
makes each name of an aliased field its own optional query parameter, because
a Parameter Object cannot express the `allOf` bounds above.

### On a variant

serde reads a variant under its wire name or any `alias` it carries, wherever
the name travels, read literally as a field's is. So every shape naming a
variant names each of them:

- The compact `enum` of an all-unit enum lists every name, each once.
- A tag property, internal or adjacent, and an externally tagged unit variant
  are an `enum` of the names in place of a `const`.
- An externally tagged object branch holds the payload under each name, bounded
  to exactly one by an `allOf` entry of a `oneOf` over `required`, as a required
  aliased field is, and so closed by `unevaluatedProperties`. It stays one
  branch, so the variant's prose and `#[deprecated]` stay in one place.

serde reads a name two variants claim as the first of them in declaration
order, and does not refuse the collision. So a name is described under the
first variant claiming it alone, never in two branches a `oneOf` would then
both match. Where the name is only a later variant's `alias`, that alias is
unreachable and dropped. Where it is a later variant's own name, serde writes
that variant under a name it reads back as the earlier one, and no schema is
true both ways: the enum is refused.

The `discriminator` maps no value, alias or not. Every branch is inline, which
implicit mapping does not consider and no `mapping` entry can name without
knowing where the schema lands in the document, so the tag property's `enum`
in each branch is what a consumer switches on.

## The order components are emitted in

A description is emitted in **insertion order**, and insertion order is fixed by
the shape of the API rather than by anything that varies between runs. Nothing
sorts: [`Map<V>`](../crates/kynos-openapi/src/lib.rs) is an `IndexMap`
throughout the model, so what goes in first comes out first.

For `components/schemas` the order is **depth-first, post-order over the router
build**: a component is registered after the descent into its own fields
finishes, so everything a type refers to is emitted before the type that refers
to it. `Registry::resolve`
([`schema/registry.rs`](../crates/kynos/src/schema/registry.rs)) is where that
falls out — it reserves the name, descends, and only then registers. A type that
refers to itself resolves through the reserved name to a `$ref`, which is what
terminates the descent rather than looping.

Everything else follows the same rule one level up: `paths` in mount order,
`tags` in declaration order, an operation's `responses` in the order the handler
and the interceptors covering it declared them.

**This is a guarantee, not an accident of the current implementation.** Beam's
acceptance contract gates its migration on byte-deterministic export, and a
conformance-fixture corpus is only worth committing if regenerating it produces
the same file. The registry, the router and the validator each keep a `HashMap`;
each is indexed and none is iterated, and
[`tests/determinism.rs`](../crates/kynos/tests/determinism.rs) is what holds
that line — it emits one fixture description in three separate processes and
compares the bytes. A second *process* rather than a second call is the point:
each process re-runs the whole build — router, registry, validation — under a
fresh hash seed, so every collection upstream of the model is rebuilt and
re-walked. A second call would reuse the same maps and agree with itself.

## Rules

| # | Rule | Enforced by |
| --- | --- | --- |
| 1 | `format` is never a field annotation | the `#[schema(...)]` grammar rejects the key |
| 2 | An emitted `format` matches what serde reads and writes | per-type unit tests over the emitted schema |
| 3 | A scalar crate is named only under `schema/impls/` | the containment greps in [`nfr.md`](nfr.md#dependencies) |
| 4 | An umbrella feature without a backend does not compile | `compile_error!` in the crate root |
| 5 | No unconstrained schema is emitted silently | absence of `Schema`, and `Unchecked` for saying so deliberately |
| 6 | `Schema` names no serde trait | the trait's own declaration, and `protobuf.rs` compiling without serde |
| 7 | An enum is described as `oneOf`, and carries a `discriminator` exactly when serde gives it a tag | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs), and `schema_tagged_enum.rs` in the pass suite |
| 8 | An untagged enum, and an untagged variant serde reads or writes, is refused; an untagged struct is left to serde | the same ledger, and `tests/ui/macros/schema_untagged_enum.rs` and `schema_untagged_variant.rs` for the wording |
| 9 | `deprecated` comes from Rust's `#[deprecated]` and nowhere else | one helper in [`derive/common.rs`](../crates/kynos-macros/src/derive/common.rs), read by the `Schema` derive and the route attribute alike |
| 10 | A description never carries `deprecated: false` | the emitters write `Some(true)` or nothing |
| 11 | A description is the same bytes in every process that emits it | [`tests/determinism.rs`](../crates/kynos/tests/determinism.rs), emitting one fixture in three processes |
| 12 | A component is registered after everything it refers to | the same file, over a known nesting chain |
| 13 | A derived error response narrows `Problem.type` to a `const`, and a status several variants share to a `oneOf` of them | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted `Responses`; the shapes themselves in [`error/problem.rs`](../crates/kynos/src/error/problem.rs) |
| 14 | A status two contributors narrow publishes both; a contributor admitting every problem document publishes for both; a shape the rule cannot place changes nothing | [`tests/description.rs`](../crates/kynos/tests/description.rs) over the document and [`tests/matrix.rs`](../crates/kynos/tests/matrix.rs) over the wire; the rule itself in [`model/response/union.rs`](../crates/kynos-openapi/src/model/response/union.rs) |
| 15 | A flattened field's schema names every member it constrains, or the field says `#[schema(open)]` and its schema's `additionalProperties`, where it has one, becomes the parent's `unevaluatedProperties` | the `Flatten` bound the `Schema` derive asserts per flattened field, snapshotted in `tests/ui/macros/schema_flatten_map.rs` and `tests/ui/traits/flatten.rs`; the emitted shape and the value it accepts in [`tests/flatten.rs`](../crates/kynos/tests/flatten.rs), against the `jsonschema` validator |
| 16 | A guard's 403 narrows to `about:blank` and the URI its scope set named, never to either alone | [`tests/description.rs`](../crates/kynos/tests/description.rs) over the document and [`tests/matrix.rs`](../crates/kynos/tests/matrix.rs) over one refusal of each shape; the const in [`security/auth.rs`](../crates/kynos/src/security/auth.rs) and what reads it in [`error/rejection.rs`](../crates/kynos/src/error/rejection.rs) |
| 17 | A described field or variant whose wire form serde's `with`, `serialize_with` or `deserialize_with` decides is refused; every newtype-struct member counts as described, whatever serde skips, and so does every other unnamed member serde does not skip both ways; a flattened `PhantomData` serde reads is refused too, though it is in no schema under rule 31, since serde hands the function its flattening serializer and deserializer, which write and demand whatever members the function names; a named field serde never reads, an unnamed member serde skips both ways outside a newtype struct, or any field of a variant serde skips both ways is left alone, and so is `serialize_with` on a variant serde reads and never writes, on its fields, or on a named field serde reads and never writes; a `#[serde(transparent)]` struct is scanned only on the one field each direction picks, the direction's single candidate under rule 23, since serde derives no direction with several and calls a function on no other field: a field picked both ways for all three keys, one picked for writing alone for `with` and `serialize_with`, one picked for reading alone for `with` and `deserialize_with`, and no field where neither direction has a single candidate | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs); `a_transparent_struct_written_through_one_field_ignores_an_unread_override` in [`tests/derives.rs`](../crates/kynos/tests/derives.rs), against serde's own derive; and `tests/ui/macros/schema_serialize_with.rs` for the wording |
| 18 | A `#[serde(other)]` catch-all on a variant serde reads is refused in every build, including one serde never writes; on a variant serde skips both ways it catches nothing and is left alone | the same ledger, and `tests/ui/macros/schema_catch_all_variant.rs` for the wording |
| 19 | `required` omits a field that is an `Option`, carries `#[serde(default)]`, or belongs to a struct carrying `#[serde(default)]`, and names every other described field that is neither flattened nor read under a distinct `alias`, including one serde reads and never writes; a flattened field is composed through `allOf` and never listed, and an aliased one is bounded under rule 34; in an object serde writes, `skip_serializing_if`, or `skip_serializing` alone, on a field serde reads is accepted only beside one of those, or on a flattened `#[schema(open)]` map under rule 20 | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema and a read of the same type |
| 20 | `skip_serializing_if`, or `skip_serializing` alone, on a non-`Option` field with no `#[serde(default)]` on the field or its struct is refused, since serde then omits the field on write and requires it on read; a field serde never reads and a flattened `PhantomData`, both in no schema, any field of a variant serde never writes, which serde only reads, and any field of a `#[serde(transparent)]` struct, which has no `required` list, are left alone; any other flattened field is decided by `#[schema(open)]` alone, since serde ignores any default on it, so an open map is exempt and every other described flattened field of an object serde writes is refused | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs); `an_open_map_that_skips_itself_when_empty_agrees_with_its_schema` in [`tests/flatten.rs`](../crates/kynos/tests/flatten.rs), against the `jsonschema` validator; and `tests/ui/macros/schema_skip_serializing_if_without_default.rs`, `tests/ui/macros/schema_skip_serializing_without_default.rs`, `tests/ui/macros/schema_skip_serializing_if_flattened_struct.rs` and `tests/ui/macros/schema_skip_serializing_if_flattened_map.rs` for the wording |
| 21 | A type carrying serde's container `into`, `from` or `try_from` is refused, struct or enum alike, since serde then writes or reads the type it names rather than the declared fields or variants; `remote` is left alone, since its fields mirror the type it names | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs), and `tests/ui/macros/schema_container_conversion.rs` for the wording |
| 22 | A `#[serde(transparent)]` struct, named or tuple, is described by the one field serde both writes and reads through, or by the single field of the one direction serde can derive for it, under that field's constraints, prose and `#[deprecated]`, with the struct's own prose and deprecation over them; it keeps its own component name, or none if generic, and `skip_serializing_if` on its field is accepted, since serde writes the field regardless | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against the value serde writes; `tests/ui/pass/schema_transparent_phantom_tuple.rs`; and the acceptance rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs) |
| 23 | A `#[serde(transparent)]` struct serde writes through one field and reads through another is refused: serde writes through the field without `skip` or `skip_serializing` and reads through the field without `skip`, `skip_deserializing` or a field-level `default`, never a `PhantomData`, and the struct is refused only when each direction picks a single field and the two differ; where only one direction picks a single field, serde refuses the other derive itself and the struct is described by that field; a container `#[serde(default)]` changes neither, and a struct where neither direction picks a single field, a unit struct or an enum is left to serde, which refuses it | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs), and `tests/ui/macros/schema_transparent_without_one_field.rs` for the wording |
| 24 | A derived tuple struct or tuple variant is a closed array whose `prefixItems` are the members serde does not skip both ways, omitted when none are, with `minItems` their count up to the last one without a field `#[serde(default)]`, since serde fills a trailing run of defaulted members when the array ends early (wherever serde writes the tuple, a last member carrying `skip_serializing_if` carries a default, its own or its tuple struct's; in a variant serde never writes it need not, and counts under rule 27), and none under a container `#[serde(default)]`, which fills every missing trailing element; a newtype struct is its member's schema whatever serde skips; a newtype variant whose member serde skips both ways is the unit variant serde writes, under external and internal tagging and in the compact `enum`, and under adjacent tagging only when the member is an `Option`, being refused otherwise, since serde writes the tag alone and reads the variant only beside its content; its member bears no `Flatten` bound | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against what serde writes and reads; the witness and `Flatten`-claim rows and the adjacent-tagging refusal's ledger row in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs); and `tests/ui/macros/schema_adjacent_skipped_payload.rs` for the refusal's wording |
| 25 | `skip_serializing` or `skip_deserializing` alone on a member of a tuple struct, tuple variant or newtype variant is refused, and so is `skip_serializing_if` on a tuple member other than a last one under a `#[serde(default)]` on it or on its tuple struct, since serde would write one shape and read another; a newtype struct, a newtype variant's `skip_serializing_if`, a `#[serde(transparent)]` struct and a variant serde skips both ways are left alone, and inside a variant serde reads and never writes only a member's lone `skip_deserializing` is refused | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs), and `tests/ui/macros/schema_tuple_member_skipped_one_way.rs` and `tests/ui/macros/schema_tuple_member_skipped_if_not_last.rs` for the wording |
| 26 | An unnamed member that is described — a newtype struct's, or one a tuple struct, tuple variant or newtype variant does not skip both ways — carries its constraints, prose and `#[deprecated]` into its schema as a named field does, beside a `$ref` where its type names a component; a member serde skips both ways outside a newtype struct contributes nothing, whatever it declares | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against the value serde writes |
| 27 | A variant serde reads and never writes, `skip_serializing` alone, is described as serde reads it: its name in the compact `enum`, or its branch, under its prose and `#[deprecated]`, and it counts toward the `Flatten` claim and its bounds as any other variant does; a variant serde skips both ways, however it is spelt, is described nowhere; its tuple's `minItems` follows rule 24, so a trailing `skip_serializing_if` without `#[serde(default)]`, which only a variant serde never writes may carry, still counts | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against what serde reads; the `Flatten`-claim, witness and acceptance rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs) |
| 28 | `#[serde(skip_deserializing)]` alone on a variant is refused, since serde writes the variant and refuses to read it back, so no closed `oneOf` or `enum` is true in both directions | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs), and `tests/ui/macros/schema_variant_skipped_on_read.rs` for the wording |
| 29 | A named field serde reads and never writes, `skip_serializing` alone, is a property under rule 19, required inside a variant serde never writes, and named beside an open map so its `unevaluatedProperties` does not reach it; a flattened open map carrying it still gives the object its `unevaluatedProperties`; a named field serde writes and never reads, `skip_deserializing` alone, is left out | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against what serde writes and reads; the acceptance rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs) |
| 30 | `#[serde(skip_deserializing)]` alone on a named field is refused in an object serde writes that `deny_unknown_fields` closes under rule 32, and beside a described `#[schema(open)]` flattened field whose type does not implement `AdmitsAny`, which a map, whose value schema is hoisted, does not unless its values are `Unchecked`, since the closing keyword or the value schema a map hoists as `unevaluatedProperties` refuses the member serde writes and the schema leaves out; an open `Unchecked` map implements it and hoists nothing, and a `HashMap` or `BTreeMap` whose values are `Unchecked` implements it and hoists the permissive schema, so the field is admitted beside either; a struct, or an internally tagged enum with a struct variant serde writes, holding such a field does not claim `Flatten`; a field serde skips both ways, an open map serde never reads and a variant serde never writes are left alone | the derive's ledger, `AdmitsAny`-witness and `Flatten`-claim rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs); `a_field_serde_never_reads_beside_an_open_unchecked_field_is_admitted` and `a_field_serde_never_reads_beside_an_open_map_of_unchecked_values_is_admitted` in [`tests/flatten.rs`](../crates/kynos/tests/flatten.rs), against the `jsonschema` validator; and `tests/ui/macros/schema_field_skipped_on_read_denying_unknown_fields.rs` for the wording of the closed object's refusal, and `tests/ui/macros/schema_field_skipped_on_read_beside_open_map.rs` for the `AdmitsAny` bound's |
| 31 | A `PhantomData` member serde does not skip is the `null` serde writes and reads for it, a property under rule 19 or a position under rule 24, without requiring `PhantomData<T>: Schema`; a flattened one, which serde neither writes into the object nor reads from it, is in no schema, and a transparent struct is never described by one | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against the value serde writes; `tests/ui/pass/schema_generic_with_phantom.rs` |
| 32 | Under `#[serde(deny_unknown_fields)]`, a struct, each struct variant's fields and each adjacently tagged branch are closed: with `additionalProperties: false` where the object composes nothing, and with `unevaluatedProperties: false` where the object carries an `allOf`, which a flattened field composes members through and an aliased field bounds its names in under rule 34. An internally tagged unit or newtype variant's tag-only object stays open, and so does a `#[serde(transparent)]` struct. An externally tagged branch that is an object admits only its variant key, with `additionalProperties: false`, or `unevaluatedProperties: false` where the variant's aliases bound its keys under rule 35, with or without the attribute. A struct, an internally tagged enum with a struct variant, and an adjacently tagged enum that the attribute closes do not claim `Flatten`, and no externally tagged enum does | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against what serde reads; `a_closed_object_admits_what_a_flattened_struct_contributes_and_nothing_else` in [`tests/flatten.rs`](../crates/kynos/tests/flatten.rs), against the `jsonschema` validator; and the `Flatten`-claim and acceptance rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs) |
| 33 | In an object `deny_unknown_fields` closes, a described `#[schema(open)]` flattened field is refused, including inside a variant serde never writes, since serde reads the map empty | the derive's ledger in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs), and `tests/ui/macros/schema_open_map_denying_unknown_fields.rs` for the wording |
| 34 | A described named field serde reads under an `alias` is a property under its wire name and each distinct alias, read literally rather than through `rename_all`; a required one carries an `allOf` entry of a `oneOf` over one `required` per name and is left out of the object's `required`, and an optional one an entry of a `not` over the `required` of its pair of names, or over an `anyOf` of one `required` per pair where there are several; so a closed object carrying one is closed by `unevaluatedProperties`, and admits the alias | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against what serde reads; `a_flattened_structs_alias_is_read_through_the_closed_parent` and `a_flattened_structs_optional_alias_is_read_through_the_closed_parent` in [`tests/flatten.rs`](../crates/kynos/tests/flatten.rs), and `an_optional_field_under_three_names_is_present_under_at_most_one` in [`tests/aliases.rs`](../crates/kynos/tests/aliases.rs), against the `jsonschema` validator; and the acceptance rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs) |
| 35 | A described variant serde reads under an `alias` is named under its wire name and each distinct alias, read literally: each once in the compact `enum`; as an `enum` in place of the `const` of a tag property or an externally tagged unit variant; and as a property of an externally tagged object branch, present under exactly one by an `allOf` entry of a `oneOf` over `required` and closed by `unevaluatedProperties`. The `discriminator` maps no value | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against what serde reads; and `an_externally_tagged_branch_is_keyed_by_exactly_one_of_its_names` in [`tests/aliases.rs`](../crates/kynos/tests/aliases.rs), against the `jsonschema` validator |
| 36 | A name two described variants claim, by their own names or an `alias`, is named under the first alone, as serde reads it: an alias an earlier variant claims is dropped from the later one, and a variant whose own name an earlier one claims is refused, since serde writes it under a name that reads back as the other; a variant serde skips both ways claims nothing | [`tests/derives.rs`](../crates/kynos/tests/derives.rs), over the emitted schema against what serde reads; `a_name_an_earlier_variant_claims_is_read_through_that_variant_alone` in [`tests/aliases.rs`](../crates/kynos/tests/aliases.rs), against the `jsonschema` validator; the derive's ledger and acceptance rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs); and `tests/ui/macros/schema_variant_name_collision.rs` for the wording |
| 37 | Under `#[serde(deny_unknown_fields)]`, a flattened field of an object rule 32 closes is bounded by `ClosedFlatten` beside `Flatten`, since serde takes a flattened key only through `deserialize_struct`: the derive implements it beside `Flatten` for a struct with no flattened field serde reads, a flattened `PhantomData` counting and one skipped both ways not, and no container `#[serde(tag = "...")]`, and for an adjacently tagged enum, never for an internally tagged one; a `#[serde(transparent)]` struct closes no object, so its flattened field is bounded by `Flatten` alone; `Box<T>` and `Arc<T>` carry it, `Problem` does not implement it, and an internally tagged newtype variant's payload is bounded by `Flatten` alone | the `ClosedFlatten`-claim and witness rows in [`derive/tests.rs`](../crates/kynos-macros/src/derive/tests.rs); `a_type_serde_reads_by_name_can_be_flattened_into_a_closed_object` in [`schema/tests.rs`](../crates/kynos/src/schema/tests.rs) for the wrappers, and a `compile_fail` doctest on the trait for `Problem`; `a_closed_object_reads_a_flattened_adjacently_tagged_enum_as_serde_does` in [`tests/flatten.rs`](../crates/kynos/tests/flatten.rs), against the `jsonschema` validator and serde's read; and `tests/ui/macros/schema_flatten_internally_tagged_denying_unknown_fields.rs` and `tests/ui/macros/schema_flatten_nested_flatten_denying_unknown_fields.rs` for the wording, and `tests/ui/macros/schema_flatten_externally_tagged_denying_unknown_fields.rs` for the two refusals a type that is not `Flatten` gets there |

## Rationale

### Why the width formats and the bounds are both emitted

They answer different readers. `uint32` tells a code generator which integer type
to declare; `minimum`/`maximum` tell a validator what to accept. Format support
is optional and generators disagree about which they honour, so emitting only one
loses information for half the ecosystem. Emitting both costs two keywords.

### Why an unregistered format is preferable to no format

The alternative for `jiff::Zoned` was a bare pattern. A pattern constrains but
does not *name*, and a consumer reading a pattern has to reverse-engineer the
intent that `date-time-zoned` states outright. Since the specification requires
unknown formats to degrade to the type alone, the pattern is still there for
anyone who ignores the name — the format is strictly additional information.

### Why two decimal backends rather than one

They are not competitors. `rust_decimal` is a fixed 96-bit mantissa with a scale
ceiling of 28, which is the right shape for money and the wrong shape for
arbitrary precision; `bigdecimal` is the reverse. Shipping one would be choosing
the user's problem for them, and both emit the same `format`, so the umbrella
carries no per-backend divergence.
