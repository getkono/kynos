//! Every way a built-in extractor can fail — one type per extractor.
//!
//! Each type names only the statuses that extractor can actually produce, and
//! [`FromRequestParts::Rejection`](crate::extract::FromRequestParts::Rejection)
//! is bound by [`Responses`], so those statuses reach the operation's
//! `responses` without an author restating them.
//!
//! # Why not one shared type
//!
//! A single union would be sound — it satisfies `emitted ⊇ observable` — and
//! would still make every operation advertise every status any extractor can
//! raise. A handler reading one path parameter would claim it might answer 401,
//! which is not a harmless over-approximation: a 401 on an endpoint with no
//! authentication is a claim a client generator turns into dead retry logic.
//!
//! Statuses raised by an interceptor rather than an extractor — 429, 503 and
//! 504 — are not here. [`RateLimit`](crate::middleware::rate_limit::RateLimit),
//! [`Concurrency`](crate::middleware::limits::Concurrency) and
//! [`Timeout`](crate::middleware::limits::Timeout) return a response directly
//! and declare it through `OperationContribution`.

use std::collections::BTreeMap;

use crate::{
    error::problem::{IntoProblem, Problem},
    http::StatusCode,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// Emits the two implementations that are mechanical for every rejection: the
/// bridge to a response, and the description built from `statuses()`.
///
/// Hand-writing fourteen identical bodies would invite one of them to drift.
/// `into_problem` stays per-type, because only it knows the variants.
macro_rules! rejection_response {
    ($rejection:ty) => {
        impl IntoResponse for $rejection {
            fn into_response(self) -> crate::http::Response {
                todo!()
            }
        }

        impl Responses for $rejection {
            fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
                let _ = registry;
                todo!()
            }
        }
    };
}

/// A path parameter did not match its declared schema.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PathRejection {
    /// The parameter could not be decoded. Produces 400.
    #[error("path parameter `{name}` is not valid")]
    Invalid {
        /// The parameter that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },
}

impl PathRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Invalid { .. } => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoProblem for PathRejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST]
    }
}

rejection_response!(PathRejection);

/// A query parameter was missing or malformed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QueryRejection {
    /// The parameter was absent or could not be decoded. Produces 400.
    #[error("query parameter `{name}` is not valid")]
    Invalid {
        /// The parameter that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },
}

impl QueryRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Invalid { .. } => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoProblem for QueryRejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST]
    }
}

rejection_response!(QueryRejection);

/// A header was missing or malformed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HeaderRejection {
    /// The header was absent or could not be decoded. Produces 400.
    #[error("header `{name}` is not valid")]
    Invalid {
        /// The header that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },
}

impl HeaderRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Invalid { .. } => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoProblem for HeaderRejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST]
    }
}

rejection_response!(HeaderRejection);

/// A cookie was missing or malformed.
///
/// Gated at item level rather than on a module, because the rest of this module
/// is reachable without the `cookie` feature and no module gate covers one type.
#[cfg(feature = "cookie")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CookieRejection {
    /// The cookie was absent or could not be decoded. Produces 400.
    #[error("cookie `{name}` is not valid")]
    Invalid {
        /// The cookie that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },
}

#[cfg(feature = "cookie")]
impl CookieRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Invalid { .. } => StatusCode::BAD_REQUEST,
        }
    }
}

#[cfg(feature = "cookie")]
impl IntoProblem for CookieRejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST]
    }
}

#[cfg(feature = "cookie")]
rejection_response!(CookieRejection);

/// The request body could not be turned into the handler's argument.
///
/// The one rejection with a genuinely wide status set, because deciding a body
/// is unacceptable happens in four distinct ways.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BodyRejection {
    /// The body was syntactically invalid. Produces 400.
    #[error("the request body could not be parsed")]
    Syntax {
        /// What was wrong with it.
        detail: String,
    },

    /// The body parsed but violated its schema. Produces 422.
    ///
    /// The split from [`Syntax`](BodyRejection::Syntax) is deliberate: a client
    /// can retry neither, but only one of them indicates a bug in its
    /// serializer.
    #[error("the request body does not satisfy its schema")]
    Schema {
        /// The failures, keyed by JSON Pointer into the body.
        failures: BTreeMap<String, String>,
    },

    /// The `Content-Type` was absent or unsupported. Produces 415.
    #[error("unsupported media type")]
    UnsupportedMediaType {
        /// What the client sent, if anything.
        received: Option<String>,
    },
    // There is deliberately no `TooLarge` variant. Capping a body is
    // `middleware::limits::BodySize`'s job, and it answers 413 through its own
    // `BodySizeExceeded` short circuit before a body extractor is reached — so
    // an extractor never meets an oversized body. A variant here would declare
    // a 413 on every operation taking a body, including the ones no `BodySize`
    // covers, which is a status the service cannot produce. `assert_conformance`
    // caught exactly that.
}

impl BodyRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Syntax { .. } => StatusCode::BAD_REQUEST,
            Self::Schema { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::UnsupportedMediaType { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        }
    }
}

impl IntoProblem for BodyRejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        &[
            StatusCode::BAD_REQUEST,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            StatusCode::UNPROCESSABLE_ENTITY,
        ]
    }
}

rejection_response!(BodyRejection);

/// No offered representation satisfied the request's `Accept`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NegotiationRejection {
    /// The `Accept` header could not be parsed. Produces 400.
    #[error("header `Accept` is not valid")]
    MalformedAccept {
        /// What was wrong with it.
        detail: String,
    },

    /// The header parsed, but nothing offered matched it. Produces 406.
    #[error("no acceptable representation")]
    NotAcceptable,
}

impl NegotiationRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::MalformedAccept { .. } => StatusCode::BAD_REQUEST,
            Self::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
        }
    }
}

impl IntoProblem for NegotiationRejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST, StatusCode::NOT_ACCEPTABLE]
    }
}

rejection_response!(NegotiationRejection);

/// A credential was absent, invalid, or insufficient.
///
/// The only rejection carrying 401 or 403, which is what keeps an endpoint with
/// no [`Auth`](crate::security::auth::Auth) argument from advertising a
/// challenge it will never send.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthRejection {
    /// Credentials were absent or invalid. Produces 401.
    #[error("authentication is required")]
    Unauthenticated,

    /// Credentials were valid but insufficient. Produces 403.
    #[error("access is not permitted")]
    Forbidden,
}

impl AuthRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
        }
    }
}

impl IntoProblem for AuthRejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN]
    }
}

rejection_response!(AuthRejection);

#[cfg(test)]
mod tests;
