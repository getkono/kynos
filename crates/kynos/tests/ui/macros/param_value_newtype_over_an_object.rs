//! A newtype's text form is its field's, so it carries one value only where
//! its field does. The control is `pass/param_value_newtype`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Window {
    from: u32,
    to: u32,
}

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
struct Range(Window);

fn main() {}
