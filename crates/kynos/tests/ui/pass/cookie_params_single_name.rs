#[derive(serde::Deserialize, kynos::CookieParams)]
struct Session {
    #[serde(rename = "sid")]
    session_id: String,
}

fn main() {}
