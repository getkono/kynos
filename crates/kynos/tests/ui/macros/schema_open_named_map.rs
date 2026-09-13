//! `#[schema(open)]` over a type whose description is a `$ref` rather than the
//! map itself: the hoist has no `additionalProperties` to take, so the referenced
//! map's value schema would stay in the `allOf` and reach `id` again.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
struct Labels(BTreeMap<String, String>);

#[derive(kynos::Schema, serde::Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: Labels,
}

fn main() {}
