//! A struct's text form is its one field's, so only a newtype derives
//! `ParamValue`: one with named fields serde writes as an object, and one
//! holding several values or none has no one field to read and write through.
//! A union is neither an enum nor a newtype. The control is
//! `pass/param_value_newtype`.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
struct Page {
    limit: u32,
}

#[derive(kynos::ParamValue)]
struct Range(u32, u32);

#[derive(kynos::ParamValue)]
struct Nothing;

#[derive(kynos::ParamValue)]
union Bits {
    int: u32,
    float: f32,
}

fn main() {}
