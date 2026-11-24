//! An internally tagged enum writes its variant under a tag, so the derive
//! gives it `SerdeTagged`.

use kynos::{Schema, schema::SerdeTagged};

#[derive(Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Update {
    Tick { at: u64 },
}

fn is_tagged<T: SerdeTagged>() {}

fn main() {
    is_tagged::<Update>();
}
