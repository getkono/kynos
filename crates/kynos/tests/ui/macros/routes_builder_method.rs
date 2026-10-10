#[kynos::get("/users")]
async fn list() {}

fn main() {
    let _ = kynos::routes![list.operation_id("listAll")];
}
