#[derive(serde::Deserialize, kynos::Schema)]
#[serde(untagged)]
enum Payload {
    Number(u32),
    Text(String),
}

fn main() {}
