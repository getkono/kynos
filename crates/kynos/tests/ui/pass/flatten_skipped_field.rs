//! A skipped field is never written, so it is described nowhere and answers to
//! no bound: a flattened map carrying `#[serde(skip)]` is not refused.

use std::collections::HashMap;

#[derive(kynos::Schema, serde::Serialize)]
struct Thing {
    id: u64,
    #[serde(skip)]
    #[serde(flatten)]
    extra: HashMap<String, String>,
}

fn main() {}
