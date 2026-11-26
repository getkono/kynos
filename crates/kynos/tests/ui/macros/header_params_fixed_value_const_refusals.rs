//! A fixed `value` naming a constant is held to the rule a literal is: the
//! derive cannot read the constant, so a `const` assertion it emits refuses
//! one holding a character a decoded header is not compared as, or outer
//! whitespace a recipient strips, at the path the attribute names.
//!
//! The control is `pass/header_params_unit_with_const_value.rs`, whose
//! constants differ only in being field values.

const ACCENTED: &str = "café";
const PADDED: &str = " v1";

#[derive(kynos::HeaderParams)]
struct Accented {
    #[header(rename = "X-Api-Version", value = ACCENTED)]
    version: (),
}

#[derive(kynos::HeaderParams)]
struct Padded {
    #[header(rename = "X-Api-Version", value = PADDED)]
    version: Option<()>,
}

fn main() {}
