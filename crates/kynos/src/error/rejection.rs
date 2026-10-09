//! Every way a built-in extractor can fail — one type per extractor.
//!
//! Each type names only the statuses that extractor can actually produce, and
//! [`FromRequestParts::Rejection`](crate::extract::FromRequestParts::Rejection)
//! is bound by [`Responses`], so those statuses reach the operation's
//! `responses` without an author restating them.
//!
//! Statuses raised by an interceptor rather than an extractor — 429, 503 and
//! 408 — are not here. [`RateLimit`](crate::middleware::rate_limit::RateLimit),
//! [`Concurrency`](crate::middleware::limits::concurrency::Concurrency) and
//! [`Timeout`](crate::middleware::limits::timeout::Timeout) return a response directly
//! and declare it through `OperationContribution`.
//!
//! # What a rejection says
//!
//! Everything a rejection carries reaches the client, so a variant holds only
//! what the request itself already determined: which parameter, which media
//! type, the limit it exceeded, where in the body a value went wrong (RFC 9457
//! §5: a problem is not a debugging channel). The one exception is the problem
//! type on [`AuthRejection::Forbidden`], which the application's authorizer
//! supplies.

use std::{collections::BTreeMap, marker::PhantomData};

use serde_json::json;

use crate::{
    error::problem::{IntoProblem, Problem, narrowed_response, problem_response},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
    security::auth::Scopes,
};

/// One response per declared status, each narrowing the shared [`Problem`]
/// component to `about:blank`, which every rejection described here writes
/// through [`Problem::new`]. [`AuthRejection`]'s 403 is the exception and
/// declares itself in [`auth_responses`].
fn problem_responses(registry: &mut Registry, statuses: &[StatusCode]) -> kynos_openapi::Responses {
    statuses
        .iter()
        .fold(kynos_openapi::Responses::new(), |responses, status| {
            responses.with(status.as_u16(), narrowed_problem(registry, *status))
        })
}

/// The response one status declares: the shared component narrowed to
/// `about:blank`, with no summary beyond the reason phrase.
fn narrowed_problem(registry: &mut Registry, status: StatusCode) -> kynos_openapi::Response {
    let problem = registry.resolve::<Problem>();

    narrowed_response(&problem, status.as_u16(), &[(None, None)])
}

/// Emits the two implementations that are mechanical for every rejection: the
/// bridge to a response, and the description built from `statuses()`.
macro_rules! rejection_response {
    ($rejection:ty) => {
        impl IntoResponse for $rejection {
            fn into_response(self) -> crate::http::Response {
                IntoProblem::into_problem(self).into_response()
            }
        }

        impl Responses for $rejection {
            fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
                problem_responses(registry, <$rejection as IntoProblem>::statuses())
            }
        }
    };
}

/// Schema failures as RFC 9457's `errors` extension, one entry per pointer.
fn pointer_errors(failures: BTreeMap<String, String>) -> Vec<serde_json::Value> {
    failures
        .into_iter()
        .map(|(pointer, detail)| json!({ "pointer": pointer, "detail": detail }))
        .collect()
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
        let status = self.status();
        let summary = self.to_string();

        match self {
            Self::Invalid { detail, .. } => {
                Problem::new(status).with_detail(format!("{summary}: {detail}"))
            }
        }
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

    /// The parameter decoded as a document that breaks a bound its schema
    /// declares. Produces 400.
    ///
    /// Raised by
    /// [`QueryString`](crate::extract::params::querystring::QueryString).
    /// 400 rather than 422, since RFC 9110's 422 is about the request's
    /// content and a query string is part of its target.
    #[error("query parameter `{name}` does not satisfy its schema")]
    Schema {
        /// The parameter that failed.
        name: String,
        /// The failures, keyed by JSON Pointer into the decoded parameter.
        failures: BTreeMap<String, String>,
    },
}

impl QueryRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Invalid { .. } | Self::Schema { .. } => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoProblem for QueryRejection {
    fn into_problem(self) -> Problem {
        let status = self.status();
        let summary = self.to_string();

        match self {
            Self::Invalid { detail, .. } => {
                Problem::new(status).with_detail(format!("{summary}: {detail}"))
            }
            // Pointers are relative to the parameter the detail names.
            Self::Schema { failures, .. } => Problem::new(status)
                .with_detail(summary)
                .with_extension("errors", pointer_errors(failures)),
        }
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
        let status = self.status();
        let summary = self.to_string();

        match self {
            Self::Invalid { detail, .. } => {
                Problem::new(status).with_detail(format!("{summary}: {detail}"))
            }
        }
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST]
    }
}

rejection_response!(HeaderRejection);

/// A cookie was missing or malformed.
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
        let status = self.status();
        let summary = self.to_string();

        match self {
            Self::Invalid { detail, .. } => {
                Problem::new(status).with_detail(format!("{summary}: {detail}"))
            }
        }
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST]
    }
}

#[cfg(feature = "cookie")]
rejection_response!(CookieRejection);

/// The request body could not be turned into the handler's argument.
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
    /// Kept apart from [`Syntax`](BodyRejection::Syntax): only a syntax error
    /// indicates a bug in the client's serializer.
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

    /// The body exceeded the limit its operation reads it under. Produces 413.
    ///
    /// Every extractor that holds a body in memory caps it, at
    /// [`DEFAULT_LIMIT`](crate::extract::body::limit::DEFAULT_LIMIT) unless a
    /// [`BodySize`](crate::middleware::limits::body_size::BodySize) covering
    /// the operation names another.
    #[error("the request body is too large")]
    TooLarge {
        /// The limit it exceeded, in bytes.
        limit: u64,
    },
}

impl BodyRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Syntax { .. } => StatusCode::BAD_REQUEST,
            Self::Schema { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::UnsupportedMediaType { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }
}

impl IntoProblem for BodyRejection {
    fn into_problem(self) -> Problem {
        let problem = Problem::new(self.status());
        let summary = self.to_string();

        match self {
            Self::Syntax { detail } => problem.with_detail(format!("{summary}: {detail}")),

            Self::Schema { failures } => problem
                .with_detail(summary)
                .with_extension("errors", pointer_errors(failures)),

            Self::UnsupportedMediaType { received } => problem.with_detail(received.map_or_else(
                || format!("{summary}: the request declared no `Content-Type`"),
                |received| format!("{summary}: `{received}`"),
            )),

            // Matches `BodySizeExceeded`, so both caps read as one refusal.
            Self::TooLarge { limit } => {
                problem.with_detail(format!("the request body exceeds {limit} bytes"))
            }
        }
    }

    fn statuses() -> &'static [StatusCode] {
        &[
            StatusCode::BAD_REQUEST,
            StatusCode::PAYLOAD_TOO_LARGE,
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
        let problem = Problem::new(self.status());
        let summary = self.to_string();

        match self {
            Self::MalformedAccept { detail } => problem.with_detail(format!("{summary}: {detail}")),
            Self::NotAcceptable => problem.with_detail(summary),
        }
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::BAD_REQUEST, StatusCode::NOT_ACCEPTABLE]
    }
}

rejection_response!(NegotiationRejection);

/// No requested byte range is satisfiable.
///
/// RFC 9110 section 14.2 answers every other unusable `Range` by ignoring it,
/// so [`Range<T>`](crate::response::range::Range) is an infallible extractor
/// and this is raised by [`Range::apply`](crate::response::range::Range::apply).
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum RangeRejection {
    /// The field was understood and no spec in it is satisfiable. Produces 416.
    #[error("no requested range is satisfiable")]
    NotSatisfiable {
        /// The length of the selected representation, which RFC 9110 section
        /// 15.5.17 asks a 416 to state.
        complete_length: u64,
    },
}

impl RangeRejection {
    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotSatisfiable { .. } => StatusCode::RANGE_NOT_SATISFIABLE,
        }
    }

    /// The `Content-Range` this rejection sends.
    #[must_use]
    pub fn content_range(&self) -> crate::response::range::headers::ContentRange {
        match *self {
            Self::NotSatisfiable { complete_length } => {
                crate::response::range::headers::ContentRange::Unsatisfied { complete_length }
            }
        }
    }
}

impl IntoProblem for RangeRejection {
    fn into_problem(self) -> Problem {
        // The complete length lives in `Content-Range` alone (RFC 9110), not
        // repeated in the body.
        Problem::new(self.status()).with_detail(self.to_string())
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::RANGE_NOT_SATISFIABLE]
    }
}

/// Adds the `Content-Range` RFC 9110 section 15.5.17 asks a 416 to carry,
/// which `rejection_response!` cannot produce.
impl IntoResponse for RangeRejection {
    fn into_response(self) -> crate::http::Response {
        let field = self.content_range();
        let mut response = IntoProblem::into_problem(self).into_response();
        crate::extract::params::header::write(response.headers_mut(), &field);
        response
    }
}

/// The 416, carrying the field it sends.
///
/// Declared through the handler's return type rather than by `Range<T>`, so
/// only operations that call [`Range::apply`](crate::response::range::Range::apply)
/// advertise it.
impl Responses for RangeRejection {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses =
            problem_responses(registry, <RangeRejection as IntoProblem>::statuses());

        let unsatisfiable = StatusCode::RANGE_NOT_SATISFIABLE.as_u16().to_string();
        if let Some(kynos_openapi::RefOr::Item(response)) =
            responses.responses.get_mut(&unsatisfiable)
        {
            response.headers.insert(
                "Content-Range".to_owned(),
                kynos_openapi::RefOr::Item(
                    crate::response::range::headers::ContentRange::unsatisfied_header(),
                ),
            );
        }

        responses
    }
}

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
    Unauthenticated {
        /// The `WWW-Authenticate` challenge this 401 sends, if the scheme has
        /// one.
        ///
        /// An [`Authenticator`](crate::security::Authenticator) leaves this
        /// `None` — use [`AuthRejection::unauthenticated`] — and
        /// [`Auth`](crate::security::auth::Auth) fills it in from
        /// [`SecurityScheme::challenge`](crate::security::SecurityScheme::challenge),
        /// so the wire and the description carry the same string.
        challenge: Option<&'static str>,
    },

    /// Credentials were valid but insufficient. Produces 403.
    #[error("access is not permitted")]
    Forbidden {
        /// The problem `type` this 403 carries, or `about:blank` when `None`.
        ///
        /// Set it with [`AuthRejection::forbidden_as`] and leave it unset with
        /// [`AuthRejection::forbidden`].
        type_uri: Option<&'static str>,
    },
}

impl AuthRejection {
    /// A 401 whose challenge has not been filled in yet.
    ///
    /// What an [`Authenticator`](crate::security::Authenticator) returns: a
    /// verifier knows the credential was unacceptable, and the scheme knows
    /// what to ask for instead.
    #[must_use]
    pub const fn unauthenticated() -> Self {
        Self::Unauthenticated { challenge: None }
    }

    /// A 403 whose type is `about:blank`.
    ///
    /// What an [`Authenticator`](crate::security::Authenticator) returns when
    /// the refusal has no name of its own.
    #[must_use]
    pub const fn forbidden() -> Self {
        Self::Forbidden { type_uri: None }
    }

    /// A 403 carrying the problem type the authorizer chose for it.
    ///
    /// The URI is the type RFC 9457 section 3.1.1 defines; the title stays the
    /// status code's reason phrase (an application wanting its own title has
    /// `#[derive(ApiError)]`).
    ///
    /// The description publishes the URI only when a
    /// [`Scoped<S, R>`](crate::security::auth::Scoped) argument's `R` names the
    /// same one as its
    /// [`FORBIDDEN_TYPE`](crate::security::auth::Scopes::FORBIDDEN_TYPE).
    /// Anywhere else — an `R` naming none, or an
    /// [`Auth<S>`](crate::security::auth::Auth) or
    /// [`MaybeAuth<S>`](crate::security::auth::MaybeAuth) argument — the
    /// declared 403 stays the shared `Problem` component.
    ///
    /// The URI is not validated and reaches the client verbatim, so it should
    /// name a class of refusal rather than a fact about the caller, and be
    /// absolute (RFC 9457 section 3.1.1). `&'static str` keeps a URI assembled
    /// from the request out; a `const` refusal is the ordinary case.
    ///
    /// ```
    /// use kynos::{error::rejection::AuthRejection, http::StatusCode};
    ///
    /// const SUSPENDED: AuthRejection =
    ///     AuthRejection::forbidden_as("https://errors.example.com/account-suspended");
    ///
    /// assert_eq!(SUSPENDED.status(), StatusCode::FORBIDDEN);
    /// ```
    #[must_use]
    pub const fn forbidden_as(type_uri: &'static str) -> Self {
        Self::Forbidden {
            type_uri: Some(type_uri),
        }
    }

    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
        }
    }

    /// The `WWW-Authenticate` challenge this rejection sends, if any.
    ///
    /// Always `None` for a 403: RFC 9110 section 15.5.2 asks for a challenge on
    /// a 401, and repeating an already-valid credential would not change the
    /// answer.
    #[must_use]
    pub fn challenge(&self) -> Option<&'static str> {
        match self {
            Self::Unauthenticated { challenge } => *challenge,
            Self::Forbidden { .. } => None,
        }
    }

    /// Sets the challenge a 401 carries, leaving a 403 alone.
    ///
    /// Replaces any challenge already set, so the one
    /// [`Auth`](crate::security::auth::Auth) supplies from the scheme is the
    /// one the description declares.
    #[must_use]
    pub fn with_challenge(self, challenge: Option<&'static str>) -> Self {
        match self {
            Self::Unauthenticated { .. } => Self::Unauthenticated { challenge },
            forbidden @ Self::Forbidden { .. } => forbidden,
        }
    }
}

impl IntoProblem for AuthRejection {
    fn into_problem(self) -> Problem {
        // Never say which check refused; the challenge travels in a header.
        let status = self.status();
        let detail = self.to_string();

        match self {
            // The title is the reason phrase `Problem::new` would give.
            Self::Forbidden {
                type_uri: Some(type_uri),
            } => Problem::of_type(status, type_uri, "Forbidden"),
            Self::Forbidden { type_uri: None } | Self::Unauthenticated { .. } => {
                Problem::new(status)
            }
        }
        .with_detail(detail)
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN]
    }
}

/// Adds the `WWW-Authenticate` field RFC 9110 section 15.5.2 requires on a
/// 401, from the challenge the rejection carries.
impl IntoResponse for AuthRejection {
    fn into_response(self) -> crate::http::Response {
        let challenge = self.challenge();
        let mut response = IntoProblem::into_problem(self).into_response();

        // `from_str` refuses a challenge that would splice a header; it is then
        // dropped rather than panicking, as `Auth`'s description also does.
        if let Some(value) = challenge.and_then(|challenge| HeaderValue::from_str(challenge).ok()) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }

        response
    }
}

/// [`AuthRejection`]'s statuses: the 401 narrowed to `about:blank`, the 403 to
/// `named` (from [`Scopes::FORBIDDEN_TYPE`](crate::security::auth::Scopes::FORBIDDEN_TYPE))
/// as well, or left wide when `None`.
pub(crate) fn auth_responses(
    registry: &mut Registry,
    named: Option<&'static str>,
) -> kynos_openapi::Responses {
    <AuthRejection as IntoProblem>::statuses().iter().fold(
        kynos_openapi::Responses::new(),
        |responses, status| {
            let declared = if *status == StatusCode::FORBIDDEN {
                forbidden(registry, *status, named)
            } else {
                narrowed_problem(registry, *status)
            };

            responses.with(status.as_u16(), declared)
        },
    )
}

/// The 403 a guard declares: a choice of two types where the scope set named
/// one, and the shared component where it did not.
///
/// Never the named URI alone: [`AuthRejection::forbidden`] stays available to
/// every authorizer, so both bodies are observable. Naming none leaves the
/// shared component, since the authorizer may then send any URI.
fn forbidden(
    registry: &mut Registry,
    status: StatusCode,
    named: Option<&'static str>,
) -> kynos_openapi::Response {
    let Some(named) = named else {
        let description = status.canonical_reason().map_or_else(
            || format!("a `{}` response", status.as_u16()),
            str::to_owned,
        );

        return problem_response(registry, description);
    };

    let problem = registry.resolve::<Problem>();

    narrowed_response(
        &problem,
        status.as_u16(),
        &[(None, None), (Some(named), None)],
    )
}

/// The rejection a [`Scoped<S, R>`](crate::security::auth::Scoped) argument
/// raises: the same two failures [`AuthRejection`] carries — it wraps one —
/// described against the scope set that demanded them.
///
/// Its 403 narrows to what [`Scopes::FORBIDDEN_TYPE`] names; naming none
/// declares exactly what `AuthRejection` does. An
/// [`Authenticator`](crate::security::Authenticator) still returns an
/// `AuthRejection`; only the guard constructs this.
pub struct ScopedRejection<R: Scopes> {
    rejection: AuthRejection,
    scopes: PhantomData<R>,
}

impl<R: Scopes> ScopedRejection<R> {
    /// Describes `rejection` against scope set `R`.
    #[must_use]
    pub const fn new(rejection: AuthRejection) -> Self {
        Self {
            rejection,
            scopes: PhantomData,
        }
    }

    /// The failure itself.
    #[must_use]
    pub fn into_inner(self) -> AuthRejection {
        self.rejection
    }
}

/// Hand-written so the scope-set marker needs no `Debug` bound.
impl<R: Scopes> std::fmt::Debug for ScopedRejection<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ScopedRejection")
            .field(&self.rejection)
            .finish()
    }
}

impl<R: Scopes> std::fmt::Display for ScopedRejection<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.rejection, f)
    }
}

/// No `source`: `Display` is already the wrapped rejection's sentence.
impl<R: Scopes> std::error::Error for ScopedRejection<R> {}

impl<R: Scopes> From<AuthRejection> for ScopedRejection<R> {
    fn from(rejection: AuthRejection) -> Self {
        Self::new(rejection)
    }
}

/// Byte for byte what the wrapped rejection writes; the scope set affects only
/// the description.
impl<R: Scopes> IntoResponse for ScopedRejection<R> {
    fn into_response(self) -> crate::http::Response {
        self.rejection.into_response()
    }
}

/// The 401 and the 403, the second narrowed to what
/// [`Scopes::FORBIDDEN_TYPE`] names.
impl<R: Scopes> Responses for ScopedRejection<R> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        auth_responses(registry, R::FORBIDDEN_TYPE)
    }
}

/// What a guard naming no scope set declares: the 403 left wide.
impl Responses for AuthRejection {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        auth_responses(registry, None)
    }
}

#[cfg(test)]
mod tests;
