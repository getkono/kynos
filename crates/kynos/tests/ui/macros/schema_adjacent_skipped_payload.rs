//! An adjacently tagged newtype variant whose member serde skips: serde writes
//! `Reading::Hidden` as `{"t":"Hidden"}` and refuses to read that back, since
//! its reader demands the content of a variant declared as a newtype. serde
//! accepts this declaration, so the refusal is the derive's own. The control is
//! `pass/schema_adjacent_skipped_optional_payload`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", content = "c")]
enum Reading {
    Count(u64),
    Hidden(#[serde(skip)] u64),
}

fn main() {}
