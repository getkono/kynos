//! An internally tagged enum writes its variant under a tag, so the derive
//! gives it `Tagged`.

use kynos::{Schema, schema::Tagged};

#[derive(Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Update {
    Tick { at: u64 },
}

fn is_tagged<T: Tagged>() {}

fn main() {
    is_tagged::<Update>();
}
