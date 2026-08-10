#[derive(kynos::Headers)]
struct Declared {
    x_request_id: String,
}

fn from_request_parts<C, T: kynos::extract::FromRequestParts<C>>() {}

fn main() {
    from_request_parts::<(), kynos::extract::params::header::Headers<Declared>>();
}
