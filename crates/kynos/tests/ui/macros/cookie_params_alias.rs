#[derive(serde::Deserialize, kynos::CookieParams)]
struct Session {
    #[serde(alias = "sid")]
    session_id: String,
}

fn main() {}
