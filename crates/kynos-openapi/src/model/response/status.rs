//! The keys of a Responses Object: exact status codes and the five wildcards.

use std::{fmt, str::FromStr};

/// The key of an entry in a [`Responses`](crate::model::response::Responses)
/// map.
///
/// Either an exact status code or one of the five permitted wildcards. No other
/// wildcard form is legal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StatusPattern {
    /// An exact status code, such as `404`.
    Code(u16),
    /// Every informational response, written `1XX`.
    Informational,
    /// Every successful response, written `2XX`.
    Success,
    /// Every redirection response, written `3XX`.
    Redirection,
    /// Every client error response, written `4XX`.
    ClientError,
    /// Every server error response, written `5XX`.
    ServerError,
}

impl StatusPattern {
    /// Whether `code` is covered by this pattern.
    #[must_use]
    pub fn matches(self, code: u16) -> bool {
        match self {
            Self::Code(exact) => exact == code,
            Self::Informational => (100..200).contains(&code),
            Self::Success => (200..300).contains(&code),
            Self::Redirection => (300..400).contains(&code),
            Self::ClientError => (400..500).contains(&code),
            Self::ServerError => (500..600).contains(&code),
        }
    }

    /// Whether this pattern is a wildcard rather than an exact code.
    #[must_use]
    pub fn is_range(self) -> bool {
        !matches!(self, Self::Code(_))
    }
}

impl fmt::Display for StatusPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code(code) => write!(f, "{code}"),
            Self::Informational => f.write_str("1XX"),
            Self::Success => f.write_str("2XX"),
            Self::Redirection => f.write_str("3XX"),
            Self::ClientError => f.write_str("4XX"),
            Self::ServerError => f.write_str("5XX"),
        }
    }
}

/// The error returned when a string is not a legal
/// [`Responses`](crate::model::response::Responses) key.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{0}` is not a valid response key: expected a status code such as `404`, \
     or one of `1XX`, `2XX`, `3XX`, `4XX`, `5XX`"
)]
pub struct InvalidStatusPattern(pub String);

impl FromStr for StatusPattern {
    type Err = InvalidStatusPattern;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.as_bytes() {
            b"1XX" => Ok(Self::Informational),
            b"2XX" => Ok(Self::Success),
            b"3XX" => Ok(Self::Redirection),
            b"4XX" => Ok(Self::ClientError),
            b"5XX" => Ok(Self::ServerError),
            // Exactly three ASCII digits, the first 1-5: the only spelling
            // `Display` renders back, unlike `u16::from_str`'s `+404`/`0404`.
            &[
                hundreds @ b'1'..=b'5',
                tens @ b'0'..=b'9',
                units @ b'0'..=b'9',
            ] => {
                let digit = |byte: u8| u16::from(byte - b'0');
                Ok(Self::Code(
                    digit(hundreds) * 100 + digit(tens) * 10 + digit(units),
                ))
            }
            _ => Err(InvalidStatusPattern(value.to_owned())),
        }
    }
}
