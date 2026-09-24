//! Arbitrary JSON flattened without `#[schema(open)]`: `Unchecked` names none
//! of the members it contributes, so it is never `Flatten`, and the refusal
//! has to say how to spell the open form instead.

#[derive(kynos::Schema, serde::Serialize)]
struct Envelope {
    id: u64,
    #[serde(flatten)]
    rest: kynos::schema::unchecked::Unchecked<serde_json::Map<String, serde_json::Value>>,
}

fn main() {}
