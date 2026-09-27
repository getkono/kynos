#[derive(serde::Deserialize, kynos::PathParams)]
struct UserPath {
    #[serde(alias = "userId")]
    user_id: u32,
}

fn main() {}
