#[derive(kynos::SecurityScheme)]
#[security(oauth2(magic_link(token_url = "https://auth.example.com/token")))]
struct Delegated;

fn main() {}
