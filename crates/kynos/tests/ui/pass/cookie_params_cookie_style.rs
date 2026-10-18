//! The control for `macros/cookie_params_style_other_than_cookie.rs`: the same
//! group, differing only in asking for the one style the derive reads as.

#[derive(kynos::CookieParams)]
struct Session {
    #[cookie(rename = "sid", style = "cookie")]
    id: String,
}

fn main() {}
