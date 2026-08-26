#[derive(kynos::SecurityScheme)]
#[security(oauth2(authorization_code(token_url = "https://auth.example.com/token")))]
struct Delegated;

fn main() {}
