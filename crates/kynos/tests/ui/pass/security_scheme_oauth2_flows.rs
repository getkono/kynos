//! The control for the five `oauth2` refusals beside it.
//!
//! Differs in exactly the property each one tests: a flow is declared, it is
//! one OAuth 2.0 defines, it carries both URLs its grant needs, no flow appears
//! twice, and no flow but the device grant declares `device_authorization_url`.
//! Both `scopes` spellings are here, since the described form is
//! what the flow map exists for.
#[derive(kynos::SecurityScheme)]
#[security(oauth2(
    authorization_code(
        authorization_url = "https://auth.example.com/authorize",
        token_url = "https://auth.example.com/token",
        refresh_url = "https://auth.example.com/token",
        scopes("users:read" = "Read a user's profile", "users:write"),
    ),
    client_credentials(token_url = "https://auth.example.com/token"),
))]
struct Delegated;

fn main() {}
