//! `alias` on a field of an object serde reads under `deny_unknown_fields`:
//! serde reads the field under a name the closed object does not name, so the
//! schema would refuse a document serde reads. The control is
//! `pass/schema_closed_without_alias`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    #[serde(alias = "identifier")]
    id: u64,
}

fn main() {}
