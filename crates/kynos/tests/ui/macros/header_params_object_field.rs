//! A header carries one value, read with `FromStr` and written with `Display`,
//! so a field whose schema is an object describes a wire form the derive never
//! reads: `simple` spreads it as `x,1,y,2`.
//!
//! Optional here, so the bound is shown to reach the `T` of an `Option<T>`,
//! which is what the derive decodes.

use std::{fmt, str::FromStr};

#[derive(kynos::Schema)]
struct Point {
    x: i32,
    y: i32,
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

#[derive(kynos::HeaderParams)]
struct Near {
    #[header(rename = "X-Near")]
    at: Option<Point>,
}

fn main() {}
