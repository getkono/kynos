#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "path", name = "key"))]
struct ApiKey;

fn main() {}
