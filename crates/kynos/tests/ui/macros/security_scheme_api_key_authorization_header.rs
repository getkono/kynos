#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header", name = "Authorization"))]
struct ApiKey;

fn main() {}
