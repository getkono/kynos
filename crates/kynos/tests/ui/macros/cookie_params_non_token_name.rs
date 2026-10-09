#[derive(serde::Deserialize, kynos::CookieParams)]
struct Session {
    #[serde(rename = "s=id")]
    session_id: String,
}

fn main() {}
