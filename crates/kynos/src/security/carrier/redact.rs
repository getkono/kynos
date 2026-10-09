//! `Debug` for the presented credentials, with every secret left out.
//!
//! A secret read from a log can be presented again. What identifies rather
//! than authenticates — a user-id, a scheme token — still prints.

use std::fmt;

use super::{ApiKey, BearerToken, Credentials, SchemeCredentials};

/// Stands in for a secret.
struct Redacted;

impl fmt::Debug for Redacted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

/// Never the token.
impl fmt::Debug for BearerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("BearerToken")
            .field(&Redacted)
            .finish()
    }
}

/// The user-id, and never the password.
impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &Redacted)
            .finish()
    }
}

/// The scheme token, and never the credentials.
impl fmt::Debug for SchemeCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SchemeCredentials")
            .field("scheme", &self.scheme)
            .field("credentials", &Redacted)
            .finish()
    }
}

/// Never the key.
impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ApiKey").field(&Redacted).finish()
    }
}
