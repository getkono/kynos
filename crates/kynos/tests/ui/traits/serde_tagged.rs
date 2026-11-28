//! An externally tagged enum writes its variant as a key rather than under a
//! tag, so the derive gives it no `SerdeTagged`.

use kynos::{Schema, schema::SerdeTagged};

#[derive(Schema, serde::Serialize)]
enum Update {
    Tick { at: u64 },
}

fn is_tagged<T: SerdeTagged>() {}

fn main() {
    is_tagged::<Update>();
}
