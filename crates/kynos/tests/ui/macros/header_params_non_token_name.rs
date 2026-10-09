#[derive(serde::Deserialize, kynos::HeaderParams)]
struct Tracing {
    #[serde(rename = "x request id")]
    request_id: String,
}

fn main() {}
