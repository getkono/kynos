#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header", name = "key"))]
struct ApiKey;

fn main() {}
