//! The same refusal where it is met in practice: a flattened map on a derived
//! type, whose schema would then constrain the properties the type declared
//! itself.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    extra: BTreeMap<String, String>,
}

fn main() {}
