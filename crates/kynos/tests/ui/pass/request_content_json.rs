#[derive(kynos::Schema, serde::Deserialize)]
struct User {
    id: u64,
}

fn is_content<T: kynos::extract::describe::RequestContent>() {}

fn main() {
    is_content::<kynos::extract::body::json::Json<User>>();
}
