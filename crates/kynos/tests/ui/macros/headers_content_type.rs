#[derive(kynos::HeaderParams)]
struct Payload {
    #[header(rename = "Content-Type")]
    content_type: String,
}

fn main() {}
