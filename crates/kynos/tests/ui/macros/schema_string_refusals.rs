//! `#[schema(string)]` describes a type as the one string `Display` writes, so
//! it refuses what would describe anything else: a flag given a value, an
//! empty `examples`, a text bound on a type that is no string, a generic
//! parameter, a field's or a variant's own bound, a key written twice, serde
//! attributes saying the type travels as its declaration or as another type,
//! a parameter group, and a `format` naming nothing. A refused derive emits
//! nothing, so no `FromStr` or `Display` is needed here; serde's conversions
//! are, and are written. The control is `pass/schema_string`.

struct Inner;

#[derive(kynos::Schema)]
#[schema(string = true)]
struct Valued(Inner);

#[derive(kynos::Schema)]
#[schema(string, examples())]
struct Unexampled(Inner);

#[derive(kynos::Schema)]
#[schema(max_length = 8)]
struct Unstringed(Inner);

#[derive(kynos::Schema)]
#[schema(string)]
struct Generic<T>(T);

#[derive(kynos::Schema)]
#[schema(string)]
struct MemberBound(#[schema(max_length = 8)] String);

#[derive(kynos::Schema)]
#[schema(string, string)]
struct Twice(Inner);

#[derive(kynos::Schema, serde::Serialize)]
#[schema(string)]
#[serde(transparent)]
struct Transparent(String);

#[derive(Clone, kynos::Schema, serde::Serialize)]
#[schema(string)]
#[serde(into = "u64")]
struct ThroughNumber(u64);

impl From<ThroughNumber> for u64 {
    fn from(number: ThroughNumber) -> Self {
        number.0
    }
}

#[derive(Clone, kynos::Schema, serde::Serialize, serde::Deserialize)]
#[schema(string)]
#[serde(into = "String")]
struct OneWay(String);

impl From<OneWay> for String {
    fn from(one: OneWay) -> Self {
        one.0
    }
}

#[derive(kynos::PathParams)]
#[schema(string)]
struct Group {
    id: u32,
}

#[derive(kynos::Schema)]
#[schema(string, format = "")]
struct Unformatted(Inner);

#[derive(kynos::Schema)]
#[schema(string)]
enum VariantBound {
    #[schema(max_length = 8)]
    One,
}

fn main() {}
