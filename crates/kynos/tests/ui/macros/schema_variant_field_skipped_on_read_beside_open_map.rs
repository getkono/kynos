//! The `AdmitsAny` bound inside an internally tagged struct variant: the
//! variant's schema leaves out the field serde never reads, and the map's
//! hoisted `unevaluatedProperties` would refuse what serde writes of it. The
//! control is `pass/schema_variant_field_skipped_on_read_beside_open_unchecked`.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
enum Event {
    Created {
        at: u64,
        #[serde(skip_deserializing)]
        stamp: u64,
        #[serde(flatten)]
        #[schema(open)]
        extra: BTreeMap<String, String>,
    },
}

fn main() {}
