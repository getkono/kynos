#[derive(kynos::HeaderParams)]
struct Payload {
    #[header(rename = "Content-Length")]
    content_length: String,
}

fn main() {}
