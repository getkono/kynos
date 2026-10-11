//! serde writes a variant declaring fields as an object keyed by its name,
//! which no parameter text holds. The control is `pass/param_value_unit_enum`.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
enum Phase {
    Checking,
    Failed { reason: String },
}

fn main() {}
