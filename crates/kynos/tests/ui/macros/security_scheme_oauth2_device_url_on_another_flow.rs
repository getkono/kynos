#[derive(kynos::SecurityScheme)]
#[security(oauth2(client_credentials(
    token_url = "https://auth.example.com/token",
    device_authorization_url = "https://auth.example.com/device"
)))]
struct Delegated;

fn main() {}
