//! A non-empty, untagged enum of unit variants serde writes and reads: the
//! control for every `macros/param_value_*` case, each of which breaks one of
//! those properties.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Checking,
    #[serde(alias = "metadata")]
    AwaitingMetadata,
}

fn param_value<T: kynos::schema::ParamValue>() {}

fn main() {
    param_value::<Phase>();
}
