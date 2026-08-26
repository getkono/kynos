fn responds<T: kynos::response::IntoResponse>() {}

fn main() {
    responds::<kynos::response::status::NoContent>();
}
