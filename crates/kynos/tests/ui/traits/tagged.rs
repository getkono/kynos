//! An externally tagged enum writes its variant as a key rather than under a
//! tag, so the derive gives it no `Tagged`.

use kynos::{Schema, schema::Tagged};

#[derive(Schema, serde::Serialize)]
enum Update {
    Tick { at: u64 },
}

fn is_tagged<T: Tagged>() {}

fn main() {
    is_tagged::<Update>();
}
