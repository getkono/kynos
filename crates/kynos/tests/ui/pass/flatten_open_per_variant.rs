//! `#[schema(open)]` is once per emitted object, and each variant of an enum is
//! an object of its own, so two variants may each carry an open field. The
//! control for the ledger case refusing a second open field in one container.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Tally {
    Counted {
        #[serde(flatten)]
        #[schema(open)]
        counts: BTreeMap<String, u64>,
    },
    Labelled {
        #[serde(flatten)]
        #[schema(open)]
        labels: BTreeMap<String, String>,
    },
}

fn main() {}
