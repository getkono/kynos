#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key"))]
struct ApiKey;

fn main() {}
