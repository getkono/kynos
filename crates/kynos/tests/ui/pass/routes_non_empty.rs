#[kynos::get("/users")]
async fn list() {}

fn main() {
    if std::hint::black_box(false) {
        let _endpoints: kynos::router::endpoint::set::Endpoints<()> = kynos::routes![list];
    }
}
