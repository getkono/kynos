#[derive(serde::Deserialize, kynos::HeaderParams)]
struct Tracing {
    #[serde(rename = "X-Request-Id")]
    request_id: String,
    #[serde(rename = "x-request-id")]
    correlation_id: String,
}

fn main() {}
