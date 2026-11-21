//! The control for `macros/schema_chrono_field_deprecated.rs`: the same
//! struct dated by jiff compiles under `deny(deprecated)`.

#![deny(deprecated)]

use kynos::Schema;

#[derive(Schema, serde::Serialize)]
pub struct Booking {
    pub on: jiff02::civil::Date,
    pub until: jiff1::civil::Date,
    pub label: String,
}

fn main() {}
