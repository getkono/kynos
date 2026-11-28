//! The control for `macros/header_params_fixed_value_const_refusals`: the same
//! groups, differing only in that each constant is a field value, one reached
//! through a module path.

const VERSION: &str = "v1";

mod policy {
    pub const NO_STORE: &str = "no-store, max-age=0";
}

#[derive(kynos::HeaderParams)]
struct Accented {
    #[header(rename = "X-Api-Version", value = VERSION)]
    version: (),
}

#[derive(kynos::HeaderParams)]
struct Padded {
    #[header(rename = "Cache-Control", value = policy::NO_STORE)]
    no_store: Option<()>,
}

fn main() {}
