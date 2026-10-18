//! The control for `macros/header_params_unknown_and_repeated_keys`: the same
//! groups, differing only in that each key is spelled the way the grammar
//! defines it and written once.

#[derive(kynos::HeaderParams)]
struct Tracing {
    #[header(rename = "X-Request-Id")]
    request_id: String,
}

#[derive(kynos::HeaderParams)]
struct Hardened {
    #[header(rename = "X-Content-Type-Options")]
    #[header(value = "nosniff")]
    nosniff: (),
}

fn main() {}
