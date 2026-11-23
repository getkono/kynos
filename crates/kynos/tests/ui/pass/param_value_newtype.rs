//! A newtype over one parameter value, as a tuple struct and as a named
//! field serde writes as itself: the control for `macros/param_value_struct_refusals`
//! and `macros/param_value_newtype_over_an_object`.

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
struct PageLimit(u32);

#[derive(kynos::Schema, kynos::ParamValue, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Cursor {
    token: String,
}

fn param_value<T: kynos::schema::ParamValue>() {}

fn main() {
    param_value::<PageLimit>();
    param_value::<Cursor>();
}
