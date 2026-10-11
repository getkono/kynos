//! A newtype has no variant names to be its text form, so it implements
//! `ParamValue` by hand. The control is `pass/param_value_unit_enum`.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
struct UserId(u64);

fn main() {}
