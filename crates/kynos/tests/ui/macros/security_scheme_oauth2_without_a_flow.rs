#[derive(kynos::SecurityScheme)]
#[security(oauth2(metadata_url = "https://auth.example.com/meta"))]
struct Delegated;

fn main() {}
