#[derive(kynos::CookieParams)]
struct Session {
    #[cookie(rename = "sid", style = "form")]
    id: String,
}

fn main() {}
