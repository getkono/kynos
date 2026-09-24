//! The control for `macros/schema_container_conversion`: the same struct,
//! differing only in that serde writes and reads it as its own fields.

#[derive(Clone, kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Celsius {
    degrees: i64,
}

impl From<Celsius> for String {
    fn from(celsius: Celsius) -> Self {
        format!("{}C", celsius.degrees)
    }
}

impl From<String> for Celsius {
    fn from(text: String) -> Self {
        Self {
            degrees: text.trim_end_matches('C').parse().unwrap_or_default(),
        }
    }
}

fn main() {}
