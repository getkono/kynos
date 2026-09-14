//! Only an internally tagged newtype variant composes its payload beside the
//! tag. An adjacently tagged one nests the payload under the content key and an
//! externally tagged one under the variant's name, so a map payload names no
//! member of the enclosing object and is not bounded by `Flatten`.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
enum Adjacent {
    Counted(BTreeMap<String, String>),
}

#[derive(kynos::Schema, serde::Serialize)]
enum External {
    Counted(BTreeMap<String, String>),
}

fn main() {}
