//! An internally tagged newtype variant composes its payload beside the tag, so
//! the payload has to name its members as a flattened field does. A map names
//! none, and its value schema would reach the tag.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Event {
    Counted(BTreeMap<String, u64>),
}

fn main() {}
