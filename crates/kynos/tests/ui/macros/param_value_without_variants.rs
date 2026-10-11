//! An enum with no variant has no value to read, so every request naming it
//! would be refused. The control is `pass/param_value_unit_enum`.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
enum Never {}

fn main() {}
