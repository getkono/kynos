//! A struct serde writes and reads as a string through `into` and `from`, whose
//! wire form its fields no longer predict: the schema would say object while
//! the body carries `"21C"`. The control is
//! `pass/schema_container_written_as_its_fields`.

#[derive(Clone, kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(into = "String", from = "String")]
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
