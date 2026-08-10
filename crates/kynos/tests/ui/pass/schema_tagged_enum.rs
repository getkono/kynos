#[derive(serde::Deserialize, kynos::Schema)]
#[serde(tag = "kind")]
enum Payload {
    Number { value: u32 },
    Text { value: String },
}

fn main() {}
