#[derive(kynos::Headers)]
struct Payload {
    #[header(rename = "Content-Type")]
    content_type: String,
}

fn main() {}
