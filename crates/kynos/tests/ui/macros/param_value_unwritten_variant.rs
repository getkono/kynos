//! A variant serde never writes leaves `Display` no name to write for it. The
//! control is `pass/param_value_unit_enum`.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
enum Phase {
    Checking,
    #[serde(skip_serializing)]
    Legacy,
}

fn main() {}
