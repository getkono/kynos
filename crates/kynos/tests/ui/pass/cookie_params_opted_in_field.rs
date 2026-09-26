//! The control for `macros/cookie_params_object_field.rs`: the same type, whose
//! schema now describes the one `x,y` value it reads and writes.

use std::{fmt, str::FromStr};

use kynos::schema::{ParamValue, Schema, registry::Registry};

struct Point {
    x: i32,
    y: i32,
}

impl Schema for Point {
    fn schema(registry: &mut Registry) -> kynos::openapi::Schema {
        String::schema(registry)
    }
}

impl FromStr for Point {
    type Err = &'static str;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (x, y) = raw.split_once(',').ok_or("expected `x,y`")?;
        Ok(Self {
            x: x.parse().map_err(|_| "x is not an integer")?,
            y: y.parse().map_err(|_| "y is not an integer")?,
        })
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.x, self.y)
    }
}

impl ParamValue for Point {}

#[derive(kynos::CookieParams)]
struct Near {
    #[cookie(rename = "near")]
    at: Point,
}

fn main() {}
