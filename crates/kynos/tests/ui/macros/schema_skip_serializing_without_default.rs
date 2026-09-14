//! `skip_serializing` alone on a field serde still requires on read: serde never
//! writes the field and refuses a request without it, so no `required` list
//! describes both. The control is `pass/schema_skip_serializing_with_default`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Draft {
    plain: u64,
    #[serde(skip_serializing)]
    elided: u64,
}

fn main() {}
