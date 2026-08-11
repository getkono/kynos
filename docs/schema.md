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
| Defined by OAS itself | `int32`, `int64`, `float`, `double`, `password` | Named in the specification ([3.1.1 §Data Type Format](../references/3.1.1.md)) |
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

Every row below is built. One caveat applies to this table and the next alike: a
leaf implementation returns its schema directly and works today, while a
composite — `Vec<T>`, a map, a tuple, a derived struct — reaches its members
through [`Registry::resolve`](../crates/kynos/src/schema/registry.rs), which is
still `todo!()`. So the shapes are settled and the composition of them is not.

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
| tuples up to twelve | `array` | — | `prefixItems`, closed with `items: false` |
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

`Binary<M>` is **designed**. Three shapes, and which one applies is decided by
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

## Rules

| # | Rule | Enforced by |
| --- | --- | --- |
| 1 | `format` is never a field annotation | the `#[schema(...)]` grammar rejects the key |
| 2 | An emitted `format` matches what serde reads and writes | per-type unit tests over the emitted schema |
| 3 | A scalar crate is named only under `schema/impls/` | the containment greps in [`nfr.md`](nfr.md#dependencies) |
| 4 | An umbrella feature without a backend does not compile | `compile_error!` in the crate root |
| 5 | No unconstrained schema is emitted silently | absence of `Schema`, and `Unchecked` for saying so deliberately |

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
