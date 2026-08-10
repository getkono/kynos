#[derive(kynos::Headers)]
struct Payload {
    #[header(rename = "Content-Length")]
    content_length: String,
}

fn main() {}
