#[derive(kynos::HeaderParams)]
struct Sniffing {
    #[header(rename = "X-Content-Type-Options")]
    nosniff: (),
}

fn main() {}
