#[derive(serde::Deserialize, kynos::PathParams)]
struct UserPath {
    #[serde(rename = "userId")]
    user_id: u32,
}

fn main() {}
