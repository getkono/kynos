//! The control for `macros/cookie_params_unknown_and_repeated_keys`: the same
//! groups, differing only in that each key is one the grammar defines and is
//! written once.

#[derive(kynos::CookieParams)]
struct Session {
    #[cookie(rename = "sid")]
    id: String,
}

#[derive(kynos::CookieParams)]
struct Theme {
    #[cookie(rename = "theme")]
    theme: String,
}

fn main() {}
