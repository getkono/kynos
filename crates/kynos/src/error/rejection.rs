//! Every way a built-in extractor can fail — and how each describes itself.
//!
//! Each variant maps to a documented status, which is what lets an extractor's
//! rejection appear in its operation's `responses` rather than being an
//! undocumented surprise.

use std::collections::BTreeMap;

use crate::{
    error::problem::{IntoProblem, Problem},
    http::StatusCode,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// Why a request could not be turned into a handler's arguments.
///
/// Every [`FromRequestParts`](crate::extract::FromRequestParts) implementation
/// rejects with one of these, and each maps to a documented status.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Rejection {
    /// A path parameter did not match its declared schema. Produces 400.
    #[error("path parameter `{name}` is not valid")]
    Path {
        /// The parameter that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },

    /// A query parameter was missing or malformed. Produces 400.
    #[error("query parameter `{name}` is not valid")]
    Query {
        /// The parameter that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },

    /// A header was missing or malformed. Produces 400.
    #[error("header `{name}` is not valid")]
    Header {
        /// The header that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },

    /// A cookie was missing or malformed. Produces 400.
    #[error("cookie `{name}` is not valid")]
    Cookie {
        /// The cookie that failed.
        name: String,
        /// What was wrong with it.
        detail: String,
    },

    /// The body was syntactically invalid. Produces 400.
    #[error("the request body could not be parsed")]
    BodySyntax {
        /// What was wrong with it.
        detail: String,
    },

    /// The body parsed but violated its schema. Produces 422.
    ///
    /// The split between this and [`BodySyntax`](Rejection::BodySyntax) is
    /// deliberate: a client can retry neither, but only one of them indicates a
    /// bug in its serializer.
    #[error("the request body does not satisfy its schema")]
    BodySchema {
        /// The failures, keyed by JSON Pointer into the body.
        failures: BTreeMap<String, String>,
    },

    /// The `Content-Type` was absent or unsupported. Produces 415.
    #[error("unsupported media type")]
    UnsupportedMediaType {
        /// What the client sent, if anything.
        received: Option<String>,
    },

    /// No offered representation satisfied `Accept`. Produces 406.
    #[error("no acceptable representation")]
    NotAcceptable,

    /// The body exceeded the configured limit. Produces 413.
    #[error("the request body is too large")]
    PayloadTooLarge {
        /// The configured maximum, in bytes.
        limit: u64,
    },

    /// Credentials were absent or invalid. Produces 401.
    #[error("authentication is required")]
    Unauthenticated,

    /// Credentials were valid but insufficient. Produces 403.
    #[error("access is not permitted")]
    Forbidden,
}

impl Rejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        todo!()
    }

    /// Every status any rejection can produce.
    #[must_use]
    pub fn all_statuses() -> &'static [StatusCode] {
        todo!()
    }
}

impl IntoProblem for Rejection {
    fn into_problem(self) -> Problem {
        todo!()
    }

    fn statuses() -> &'static [StatusCode] {
        Self::all_statuses()
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> crate::http::Response {
        todo!()
    }
}

impl Responses for Rejection {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
