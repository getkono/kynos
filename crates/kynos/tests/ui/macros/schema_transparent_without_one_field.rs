//! A transparent struct serde writes through both fields and reads through
//! `value` alone, since `extra` has a default: no one field is both. serde
//! accepts this declaration for `Deserialize`, so the
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
