//! An untagged variant: serde writes `Reading::Bare(7)` as `7`, and reads it
//! only once every tagged variant has failed, so a branch keyed by `Bare`
//! describes a value the wire never carries. serde accepts this declaration,
//! so the refusal is the derive's own.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
enum Reading {
    Labelled { value: u64 },
    #[serde(untagged)]
    Bare(u64),
}

fn main() {}
