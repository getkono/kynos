//! A `#[schema(string)]` type carrying `FromStr` and `Display`, its serde
//! either converting through `String` both ways or written by hand with no
//! serde attribute. The control of `macros/schema_string_refusals` and
//! `macros/schema_string_without_from_str`.

use std::{fmt, str::FromStr};

/// A slug, converted through `String` by serde's derive.
#[derive(Clone, kynos::Schema, serde::Serialize, serde::Deserialize)]
#[schema(string, min_length = 1, examples("ada"))]
#[serde(into = "String", try_from = "String")]
struct Slug(Vec<u8>);

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

impl FromStr for Slug {
    type Err = std::convert::Infallible;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self(text.as_bytes().to_vec()))
    }
}

impl TryFrom<String> for Slug {
    type Error = std::convert::Infallible;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Slug> for String {
    fn from(slug: Slug) -> Self {
        slug.to_string()
    }
}

/// A count travelling as text, its serde written by hand.
#[derive(kynos::Schema)]
#[schema(string)]
struct Count(u32);

impl fmt::Display for Count {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for Count {
    type Err = std::num::ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse().map(Self)
    }
}

impl serde::Serialize for Count {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

fn main() {}
