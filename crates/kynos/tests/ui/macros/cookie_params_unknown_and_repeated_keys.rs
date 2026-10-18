//! The `#[cookie(...)]` grammar is closed and reads each key once: `value`
//! fixes a header's value and is no cookie key, and a second `rename` silently
//! replaced the first.

#[derive(kynos::CookieParams)]
struct Session {
    #[cookie(rename = "sid", value = "anonymous")]
    id: String,
}

#[derive(kynos::CookieParams)]
struct Theme {
    #[cookie(rename = "theme", rename = "colour")]
    theme: String,
}

fn main() {}
