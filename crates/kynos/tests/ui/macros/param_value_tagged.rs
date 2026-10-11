//! An internally tagged enum writes even a unit variant as an object holding
//! the tag. The control is `pass/param_value_unit_enum`.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
enum Phase {
    Checking,
    Seeding,
}

fn main() {}
