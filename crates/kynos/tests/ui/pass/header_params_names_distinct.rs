//! The control for `macros/header_params_names_differing_in_case`: the same
//! two fields, under names that differ in more than case.

#[derive(serde::Deserialize, kynos::HeaderParams)]
struct Tracing {
    #[serde(rename = "X-Request-Id")]
    request_id: String,
    #[serde(rename = "x-correlation-id")]
    correlation_id: String,
}

fn main() {}
