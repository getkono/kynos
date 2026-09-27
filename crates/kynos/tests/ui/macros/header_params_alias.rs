#[derive(serde::Deserialize, kynos::HeaderParams)]
struct Tracing {
    #[serde(alias = "x-request-id")]
    request_id: String,
}

fn main() {}
