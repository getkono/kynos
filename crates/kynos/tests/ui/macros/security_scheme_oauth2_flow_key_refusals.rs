//! A flow's keys are the ones OAuth 2.0 defines for it, each written once: a
//! misspelled `token_url` is refused rather than dropped, and a second one
//! rather than left to replace the first.
//!
//! The control is `pass/security_scheme_oauth2_flows.rs`.

#[derive(kynos::SecurityScheme)]
#[security(oauth2(client_credentials(
    token_url = "https://auth.example.com/token",
    token_uri = "https://auth.example.com/token",
)))]
struct Misspelled;

#[derive(kynos::SecurityScheme)]
#[security(oauth2(client_credentials(
    token_url = "https://auth.example.com/a",
    token_url = "https://auth.example.com/b",
)))]
struct Twice;

fn main() {}
