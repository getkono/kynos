//! A `#[serde(other)]` catch-all, which accepts every tag the enum does not
//! name while the schema's `oneOf` lists only the named ones. The control is
//! `pass/schema_enum_naming_every_tag`.

#[derive(kynos::Schema, serde::Deserialize)]
#[serde(tag = "kind")]
enum Event {
    Created { id: u64 },
    Deleted { id: u64 },
    #[serde(other)]
    Unknown,
}

fn main() {}
