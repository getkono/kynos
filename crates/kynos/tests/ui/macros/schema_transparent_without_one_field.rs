//! A transparent struct serde reads through `value` alone, since `extra` has a
//! default, while `Schema` describes both fields and so cannot pick the one
//! serde reads. serde accepts this declaration for `Deserialize`, so the
//! refusal is the derive's own. The control is
//! `pass/schema_transparent_one_field`.

#[derive(kynos::Schema, serde::Deserialize)]
#[serde(transparent)]
struct Reading {
    value: u64,
    #[serde(default)]
    extra: String,
}

fn main() {}
