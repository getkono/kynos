//! A variant serde writes and never reads: `Channel::Fax` writes `"Fax"`, which
//! serde refuses to read back, so no closed `enum` is true of both. serde
//! accepts this declaration, so the refusal is the derive's own. The control is
//! `pass/schema_variant_skipped_both_ways`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
enum Channel {
    Web,
    #[serde(skip_deserializing)]
    Fax,
}

fn main() {}
