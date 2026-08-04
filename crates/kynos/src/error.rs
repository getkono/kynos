//! Errors, and the one recommended way to represent them on the wire.
//!
//! Kynos uses [RFC 9457 problem details] for every error it produces, and
//! `#[derive(ApiError)]` produces them for yours. One shape across the whole
//! description means a client can handle failures generically instead of
//! learning a different envelope per endpoint.
//!
//! Crucially, this covers the framework's *own* rejections. When a body fails
//! to parse, or a path parameter will not deserialize, the resulting 400 is a
//! problem document and it appears in the operation's `responses` — because
//! [`FromRequestParts::Rejection`](crate::extract::FromRequestParts::Rejection)
//! is required to describe itself. No other Rust framework documents its
//! extractor rejections at all.
//!
//! [RFC 9457 problem details]: https://www.rfc-editor.org/rfc/rfc9457

use std::{borrow::Cow, collections::BTreeMap};

use serde_json::Value;

use crate::{
    http::StatusCode,
    response::{IntoResponse, Responses},
    schema::Registry,
};

/// The result type used throughout the framework.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A failure raised by the framework itself, not by a handler.
///
/// These surface while a router is being built or a server started — never
/// while serving a request, where a [`Problem`] is returned instead.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The router describes an API that OpenAPI cannot express, or expresses
    /// incorrectly.
    #[error("the router does not describe a valid API")]
    Invalid {
        /// Every violation found, most structural first.
        violations: Vec<kynos_openapi::Violation>,
    },

    /// A path template was malformed, or collided with another.
    #[error(transparent)]
    Path(#[from] kynos_openapi::paths::InvalidPathTemplate),

    /// Two types claimed the same component name.
    #[error(transparent)]
    Schema(#[from] crate::schema::SchemaConflict),

    /// The document could not be serialized.
    #[error("the description could not be serialized")]
    Serialize(#[from] serde_json::Error),

    /// The listener could not be bound, or the server could not start.
    #[error("the server could not start")]
    Io(#[from] std::io::Error),

    /// The server configuration or transport failed.
    #[cfg(feature = "server")]
    #[error(transparent)]
    Server(#[from] crate::server::ServerError),
}

/// An RFC 9457 problem detail.
///
/// The five registered members are typed; anything else goes in
/// [`extensions`](Problem::extensions), which is how an error carries the
/// specifics a client needs to act on it — which field failed, which quota was
/// exceeded, when to retry.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    /// A URI identifying the problem *type*.
    ///
    /// Defaults to `about:blank`, which means "the status code is the whole
    /// story". Anything a client should branch on deserves a real URI.
    pub type_uri: Cow<'static, str>,

    /// A short, human-readable summary of the problem type.
    ///
    /// Should not change from occurrence to occurrence; put the specifics in
    /// [`detail`](Problem::detail).
    pub title: Cow<'static, str>,

    /// The HTTP status code.
    pub status: StatusCode,

    /// An explanation specific to this occurrence.
    pub detail: Option<String>,

    /// A URI identifying this specific occurrence.
    pub instance: Option<String>,

    /// Additional members, serialized alongside the registered ones.
    pub extensions: BTreeMap<String, Value>,
}

impl Problem {
    /// Creates a problem with `about:blank` as its type.
    #[must_use]
    pub fn new(status: StatusCode) -> Self {
        let _ = status;
        todo!()
    }

    /// Creates a problem with an identifying type URI and title.
    #[must_use]
    pub fn of_type(
        status: StatusCode,
        type_uri: impl Into<Cow<'static, str>>,
        title: impl Into<Cow<'static, str>>,
    ) -> Self {
        let _ = (status, type_uri, title);
        todo!()
    }

    /// Sets the occurrence-specific explanation.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let _ = &mut self;
        let _ = detail;
        todo!()
    }

    /// Sets the URI identifying this occurrence.
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        let _ = &mut self;
        let _ = instance;
        todo!()
    }

    /// Attaches an additional member.
    #[must_use]
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        let _ = &mut self;
        let _ = (key, value);
        todo!()
    }
}

/// A type that becomes an error response.
///
/// Derive it with `#[derive(ApiError)]`. The derive maps each variant to a
/// status and a problem type, and — this is the part that matters — emits the
/// [`Responses`] implementation at the same time, so the statuses an error can
/// produce and the statuses the description advertises cannot disagree.
///
/// ```no_run
/// # use kynos::error::IntoProblem;
/// # #[derive(Debug)] struct UserId(u64);
/// #[derive(Debug, thiserror::Error)]
/// enum ApiError {
///     #[error("no user with id {0:?}")]
///     NotFound(UserId),
///     #[error("that email is already registered")]
///     EmailTaken,
/// }
/// # impl IntoProblem for ApiError {
/// #     fn into_problem(self) -> kynos::Problem { todo!() }
/// #     fn statuses() -> &'static [kynos::http::StatusCode] { todo!() }
/// # }
/// ```
pub trait IntoProblem {
    /// Converts this error into its wire representation.
    fn into_problem(self) -> Problem;

    /// Every status this type can produce.
    ///
    /// The [`Responses`] implementation is derived from this, so a status
    /// returned at runtime but missing here is a bug the description would
    /// hide. The derive computes it; hand implementations must keep it honest.
    fn statuses() -> &'static [StatusCode];
}

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

impl IntoResponse for Problem {
    fn into_response(self) -> crate::http::Response {
        todo!()
    }
}

impl Responses for Problem {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
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
