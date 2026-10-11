#[derive(kynos::HeaderParams)]
struct Sniffing {
    #[header(rename = "X-Content-Type-Options", value = "nosniff")]
    nosniff: (),
}

fn main() {}
