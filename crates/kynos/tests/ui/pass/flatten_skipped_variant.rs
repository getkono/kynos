//! A skipped variant is never written, so the derive describes no branch for it
//! and what it declares answers to no bound: neither a flattened map among its
//! fields nor a map as its newtype payload is refused.

use std::collections::{BTreeMap, HashMap};

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Event {
    Created {
        at: String,
    },
    #[serde(skip)]
    Internal {
        id: u64,
        #[serde(flatten)]
        extra: HashMap<String, String>,
    },
    #[serde(skip)]
    Raw(BTreeMap<String, u64>),
}

fn main() {}
