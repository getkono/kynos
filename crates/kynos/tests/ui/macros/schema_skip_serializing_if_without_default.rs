//! `skip_serializing_if` on a field serde still requires on read: a response
//! may omit it, a request may not, so no `required` list describes both. The
//! control is `pass/schema_skip_serializing_if_with_default`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Draft {
    plain: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    elided: String,
}

fn main() {}
