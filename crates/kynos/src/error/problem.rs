//! RFC 9457 problem details: the one error shape Kynos puts on the wire.

use std::{borrow::Cow, collections::BTreeMap};

use serde_json::Value;

use crate::{
    http::StatusCode,
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// An RFC 9457 problem detail.
///
/// The five registered members are typed; anything else goes in
/// [`extensions`](Problem::extensions), which is how an error carries the
/// specifics a client needs to act on it — which field failed, which quota was
/// exceeded, when to retry.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
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
/// # use kynos::error::problem::IntoProblem;
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

/// Serialized by hand rather than derived: [`StatusCode`] is not
/// [`serde::Serialize`], `type_uri` is written as RFC 9457's `type`, and the
/// extension members are flattened alongside the registered ones rather than
/// nested under a field of their own.
impl serde::Serialize for Problem {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let _ = serializer;
        todo!()
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
