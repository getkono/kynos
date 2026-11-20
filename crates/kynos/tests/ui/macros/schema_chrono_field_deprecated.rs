//! A chrono field warns under `time-chrono04`, spanned at the field's type,
//! and that warning reaches the application through the derive's expansion:
//! `deny(deprecated)` turns it into the error recorded here. The control is
//! `pass/schema_jiff_field_not_deprecated.rs`, the same struct on jiff.

#![deny(deprecated)]

use chrono04 as chrono;
use kynos::Schema;

#[derive(Schema, serde::Serialize)]
pub struct Booking {
    pub on: chrono::NaiveDate,
    pub label: String,
}

fn main() {}
