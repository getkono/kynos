#[derive(kynos::SecurityScheme)]
#[security(oauth2(
    client_credentials(token_url = "https://auth.example.com/a"),
    client_credentials(token_url = "https://auth.example.com/b"),
))]
struct Delegated;

fn main() {}
