//! Every way a built-in extractor can fail — one type per extractor.
//!
//! Each type names only the statuses that extractor can actually produce, and
//! every extractor's rejection is bound by [`Responses`], so those statuses
//! reach the operation's `responses` without an author restating them — see
//! [Rejections describe themselves too](crate::extract#rejections-describe-themselves-too).
//!
//! Statuses an interceptor refuses with rather than an extractor are not here:
//! an interceptor declares them through its
//! [`Interceptor::Short`](crate::middleware::Interceptor::Short).
//!
//! # What a rejection says
//!
//! Everything a rejection carries reaches the client, so a variant holds only
//! what the request itself already determined: which parameter, which media
//! type, the limit it exceeded, where in the body a value went wrong (RFC 9457
//! §5: a problem is not a debugging channel). The one exception is the problem
//! type on [`AuthRejection::Forbidden`], which the application's authorizer
//! supplies.

use std::{collections::BTreeMap, marker::PhantomData, time::Duration};

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};
use serde_json::json;

use crate::{
    error::problem::{IntoProblem, Member, Problem, narrowed_response, problem_response},
    http::{HeaderValue, StatusCode, header, retry_after_header, set_retry_after},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
    security::{SecurityScheme, auth::Scopes},
};

/// One response per declared status, each narrowing the shared [`Problem`]
/// component to `about:blank`, which every rejection described here writes
/// through [`Problem::new`]. [`AuthRejection`]'s 403 is the exception and
/// declares itself in [`auth_responses`].
///
/// `errors` names the status a schema failure answers with, if any, and
/// whether every response at it sends the [`pointer_errors`] member.
fn problem_responses(
    registry: &mut Registry,
    statuses: &[StatusCode],
    errors: Option<(StatusCode, bool)>,
) -> kynos_openapi::Responses {
    statuses
        .iter()
        .fold(kynos_openapi::Responses::new(), |responses, status| {
            let members: Vec<Member> = errors
                .filter(|(at, _)| at == status)
                .map(|(_, sent)| ("errors", pointer_errors_schema(registry), sent))
                .into_iter()
                .collect();

            responses.with(
                status.as_u16(),
                narrowed_problem(registry, *status, &members),
            )
        })
}

/// The response one status declares: the shared component narrowed to
/// `about:blank` and sending `members`, with no summary beyond the reason
/// phrase.
fn narrowed_problem(
    registry: &mut Registry,
    status: StatusCode,
    members: &[Member],
) -> kynos_openapi::Response {
    let problem = registry.resolve::<Problem>();

    narrowed_response(&problem, status.as_u16(), &[(None, None, members)])
}

/// Emits the two implementations that are mechanical for every rejection: the
/// bridge to a response, and the description built from `statuses()`.
///
/// `errors: STATUS => SENT` declares the [`pointer_errors`] member under the
/// status a schema failure answers with, required where `SENT` says every
/// response at that status carries it.
macro_rules! rejection_response {
    (@[$errors:expr] $rejection:ty) => {
        impl IntoResponse for $rejection {
            fn into_response(self) -> crate::http::Response {
                IntoProblem::into_problem(self).into_response()
            }
        }

        impl Responses for $rejection {
            fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
                problem_responses(registry, <$rejection as IntoProblem>::statuses(), $errors)
            }
        }
    };
    ($rejection:ty, errors: $status:expr => $sent:expr) => {
        rejection_response!(@[Some(($status, $sent))] $rejection);
    };
    ($rejection:ty) => {
        rejection_response!(@[None] $rejection);
    };
}

/// Schema failures as RFC 9457's `errors` extension, one entry per pointer.
fn pointer_errors(failures: BTreeMap<String, String>) -> Vec<serde_json::Value> {
    failures
        .into_iter()
        .map(|(pointer, detail)| json!({ "pointer": pointer, "detail": detail }))
        .collect()
}

/// What [`pointer_errors`] sends, declared: an array of `{pointer, detail}`
/// objects, each pointer a JSON Pointer (RFC 6901).
fn pointer_errors_schema(registry: &mut Registry) -> OpenApiSchema {
    let detail = registry.resolve::<String>();
    let mut pointer = detail.clone();
    if let OpenApiSchema::Object(object) = &mut pointer {
        object.format = Some("json-pointer".to_owned());
    }

    let entry = SchemaObject {
        ty: Some(TypeSet::One(SchemaType::Object)),
        properties: [
            ("pointer".to_owned(), pointer),
            ("detail".to_owned(), detail),
        ]
        .into_iter()
        .collect(),
        required: Some(vec!["pointer".to_owned(), "detail".to_owned()]),
        ..SchemaObject::default()
    };

    OpenApiSchema::Object(Box::new(SchemaObject {
        ty: Some(TypeSet::One(SchemaType::Array)),
        items: Some(Box::new(OpenApiSchema::Object(Box::new(entry)))),
        ..SchemaObject::default()
    }))
}

/// Declares a parameter rejection: an `Invalid` and a `Schema` variant, both
/// 400, over the `subject` ("parameter", "header", ...) its field docs name.
///
/// A leading `#[cfg(...)]` gates the type and every implementation with it.
/// Each invocation spells its own derive, so a sweep reading declarations from
/// source finds every rejection by name.
macro_rules! parameter_rejection {
    (#[cfg($cfg:meta)] $($rest:tt)*) => {
        parameter_rejection!(@[$cfg] $($rest)*);
    };
    (
        @[$($cfg:meta)?]
        subject: $subject:literal,
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(#[$invalid:meta])*
            Invalid,
            $(#[$schema:meta])*
            Schema $(,)?
        }
    ) => {
        $(#[cfg($cfg)])?
        $(#[$meta])*
        pub enum $name {
            $(#[$invalid])*
            Invalid {
                #[doc = concat!("The ", $subject, " that failed.")]
                name: String,
                /// What was wrong with it.
                detail: String,
            },

            $(#[$schema])*
            Schema {
                #[doc = concat!("The ", $subject, " that failed.")]
                name: String,
                #[doc = concat!(
                    "The failures, keyed by JSON Pointer into the decoded ",
                    $subject,
                    "."
                )]
                failures: BTreeMap<String, String>,
            },
        }

        $(#[cfg($cfg)])?
        impl $name {
            /// The status this rejection produces.
            #[must_use]
            pub fn status(&self) -> StatusCode {
                match self {
                    Self::Invalid { .. } | Self::Schema { .. } => StatusCode::BAD_REQUEST,
                }
            }
        }

        $(#[cfg($cfg)])?
        impl IntoProblem for $name {
            fn into_problem(self) -> Problem {
                let status = self.status();
                let summary = self.to_string();

                match self {
                    Self::Invalid { detail, .. } => {
                        Problem::new(status).with_detail(format!("{summary}: {detail}"))
                    }
                    // Pointers are relative to the subject the detail names.
                    Self::Schema { failures, .. } => Problem::new(status)
                        .with_detail(summary)
                        .with_extension("errors", pointer_errors(failures)),
                }
            }

            fn statuses() -> &'static [StatusCode] {
                &[StatusCode::BAD_REQUEST]
            }
        }

        $(#[cfg($cfg)])?
        rejection_response!($name, errors: StatusCode::BAD_REQUEST => false);
    };
    ($($rest:tt)*) => {
        parameter_rejection!(@[] $($rest)*);
    };
}

parameter_rejection! {
    subject: "parameter",
    /// A path parameter did not match its declared schema.
    #[derive(Debug, thiserror::Error)]
    #[non_exhaustive]
    pub enum PathRejection {
        /// The parameter could not be decoded. Produces 400.
        #[error("path parameter `{name}` is not valid")]
        Invalid,

        /// The parameter decoded as a value that breaks a bound its schema
        /// declares. Produces 400.
        ///
        /// Raised by a derived `PathParams` for a field's `#[schema(...)]` bound or
        /// its type's. 400 rather than 422, as for a query parameter: a path is part
        /// of the request's target, not its content.
        #[error("path parameter `{name}` does not satisfy its schema")]
        Schema,
    }
}

parameter_rejection! {
    subject: "parameter",
    /// A query parameter was missing or malformed.
    #[derive(Debug, thiserror::Error)]
    #[non_exhaustive]
    pub enum QueryRejection {
        /// The parameter was absent or could not be decoded. Produces 400.
        #[error("query parameter `{name}` is not valid")]
        Invalid,

        /// The parameter decoded as a value that breaks a bound its schema
        /// declares. Produces 400.
        ///
        /// Raised by
        /// [`QueryString`](crate::extract::params::querystring::QueryString), and
        /// by [`Query`](crate::extract::params::query::Query) over a derived
        /// `QueryParams` for a field's `#[schema(...)]` bound or its type's.
        /// 400 rather than 422, since RFC 9110's 422 is about the request's
        /// content and a query string is part of its target.
        #[error("query parameter `{name}` does not satisfy its schema")]
        Schema,
    }
}

parameter_rejection! {
    subject: "header",
    /// A header was missing or malformed.
    #[derive(Debug, thiserror::Error)]
    #[non_exhaustive]
    pub enum HeaderRejection {
        /// The header was absent or could not be decoded. Produces 400.
        #[error("header `{name}` is not valid")]
        Invalid,

        /// The header decoded as a value that breaks a bound its schema
        /// declares. Produces 400.
        ///
        /// Raised by a derived `HeaderParams` for a field's `#[schema(...)]` bound
        /// or its type's. 400 rather than 422, since RFC 9110's 422 is about the
        /// request's content and a header field is not part of it.
        #[error("header `{name}` does not satisfy its schema")]
        Schema,
    }
}

parameter_rejection! {
    #[cfg(feature = "cookie")]
    subject: "cookie",
    /// A cookie was missing or malformed.
    #[derive(Debug, thiserror::Error)]
    #[non_exhaustive]
    pub enum CookieRejection {
        /// The cookie was absent or could not be decoded. Produces 400.
        #[error("cookie `{name}` is not valid")]
        Invalid,

        /// The cookie decoded as a value that breaks a bound its schema
        /// declares. Produces 400.
        ///
        /// Raised by a derived `CookieParams` for a field's `#[schema(...)]` bound
        /// or its type's. 400 rather than 422, since a cookie travels in a header
        /// field rather than the request's content.
        #[error("cookie `{name}` does not satisfy its schema")]
        Schema,
    }
}

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

    /// The body is still under a content coding: `Content-Encoding` names one
    /// besides `identity`. Produces 415.
    ///
    /// `Decompression` removes the field once it decodes, so a coding a body
    /// extractor still sees is one nothing on its operation decoded (RFC 9110
    /// sections 8.4 and 15.5.16).
    #[error("the request body's content coding was not decoded")]
    UnsupportedCoding {
        /// The `Content-Encoding` the client sent, its fields joined by `, `.
        received: String,
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
            Self::UnsupportedMediaType { .. } | Self::UnsupportedCoding { .. } => {
                StatusCode::UNSUPPORTED_MEDIA_TYPE
            }
            Self::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }

    /// The 422 for one failure, keyed at `pointer`; the empty pointer for a
    /// decoder that names no location.
    pub(crate) fn schema_at(pointer: String, detail: String) -> Self {
        Self::Schema {
            failures: BTreeMap::from([(pointer, detail)]),
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

            Self::UnsupportedCoding { received } => problem.with_detail(format!(
                "{summary}: `{received}`; send the body without a content coding"
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

rejection_response!(BodyRejection, errors: StatusCode::UNPROCESSABLE_ENTITY => true);

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
        crate::response::headers::write_or_refuse(&mut response, &field);
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
            problem_responses(registry, <RangeRejection as IntoProblem>::statuses(), None);

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

/// A credential was absent, invalid, or insufficient, or could not be checked.
///
/// The only rejection carrying 401 or 403, which is what keeps an endpoint with
/// no [`Auth`](crate::security::auth::Auth) argument from advertising a
/// challenge it will never send. Its 503 is declared only by a guard whose
/// scheme opts in through
/// [`SecurityScheme::MAY_BE_UNAVAILABLE`].
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
        /// [`SecurityScheme::challenge`],
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

        /// The `WWW-Authenticate` challenge this 403 sends, if it refused a
        /// scope and the scheme has a challenge for that.
        ///
        /// An [`Authenticator`](crate::security::Authenticator) leaves this
        /// `None`, as both constructors do, and a guard replaces it: a refusal
        /// from [`Authenticator::authorize`](crate::security::Authenticator::authorize)
        /// carries
        /// [`SecurityScheme::insufficient_scope_challenge`],
        /// and one from
        /// [`Authenticator::authenticate`](crate::security::Authenticator::authenticate)
        /// carries none, so the wire and the description carry the same string.
        challenge: Option<&'static str>,
    },

    /// What a credential is checked against could not be reached. Produces
    /// 503.
    ///
    /// Says nothing about the credential, so a client keeps it and retries
    /// rather than discarding it as it would on a 401. Built only through
    /// [`AuthRejection::unavailable`], which a scheme that has not opted in
    /// through [`SecurityScheme::MAY_BE_UNAVAILABLE`]
    /// refuses at build time, so only an operation declaring the 503 can send
    /// it.
    #[error("authentication is temporarily unavailable")]
    #[non_exhaustive]
    Unavailable {
        /// How long the client should wait before retrying, sent as
        /// `Retry-After` in whole seconds, rounded up (RFC 9110 section
        /// 10.2.3). Set it with [`AuthRejection::with_retry_after`].
        retry_after: Option<Duration>,
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
        Self::Forbidden {
            type_uri: None,
            challenge: None,
        }
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
            challenge: None,
        }
    }

    /// A 503: the credential store scheme `S` checks against is unreachable.
    ///
    /// What an [`Authenticator`](crate::security::Authenticator) returns when
    /// it cannot tell whether the credential is valid — a session table whose
    /// database is down, an introspection endpoint timing out. Answering 401
    /// instead would tell a browser to sign its user out over a transient
    /// outage.
    ///
    /// `S` is the scheme the authenticator checks, and it must opt in with
    /// [`SecurityScheme::MAY_BE_UNAVAILABLE`]
    /// (`#[security(unavailable)]` on a derived one), which is what declares
    /// the 503 on every operation it guards. The 503 is declared only there:
    /// returned from a handler or custom guard on an operation no guard over
    /// `S` covers, it is a status the description does not name. Naming a
    /// scheme that has not opted in fails the build:
    ///
    /// ```
    /// # #[cfg(feature = "macros")]
    /// # {
    /// use kynos::{SecurityScheme, error::rejection::AuthRejection, http::StatusCode};
    ///
    /// #[derive(SecurityScheme)]
    /// #[security(api_key(in = "cookie", name = "session"), unavailable)]
    /// struct Session;
    ///
    /// let down = AuthRejection::unavailable::<Session>()
    ///     .with_retry_after(std::time::Duration::from_secs(5));
    /// assert_eq!(down.status(), StatusCode::SERVICE_UNAVAILABLE);
    /// # }
    /// ```
    ///
    /// ```compile_fail,E0080
    /// use kynos::{error::rejection::AuthRejection, security::schemes::Bearer};
    ///
    /// let _ = AuthRejection::unavailable::<Bearer>();
    /// ```
    #[must_use]
    pub const fn unavailable<S: SecurityScheme>() -> Self {
        const {
            assert!(
                S::MAY_BE_UNAVAILABLE,
                "this scheme does not declare a 503: set `SecurityScheme::MAY_BE_UNAVAILABLE`, \
                 or `#[security(unavailable)]` on a derived scheme"
            );
        }
        Self::Unavailable { retry_after: None }
    }

    /// Sets the `Retry-After` a 503 sends, leaving a 401 or a 403 alone.
    ///
    /// RFC 9110 section 10.2.3 lets a 503 say how long the outage is expected
    /// to last; the delay is sent in whole seconds, rounded up, so a
    /// sub-second wait is never sent as `0`.
    #[must_use]
    pub const fn with_retry_after(self, delay: Duration) -> Self {
        match self {
            Self::Unavailable { .. } => Self::Unavailable {
                retry_after: Some(delay),
            },
            other => other,
        }
    }

    /// The `Retry-After` delay a 503 sends, if any; `None` on a 401 or a 403.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Unavailable { retry_after } => *retry_after,
            Self::Unauthenticated { .. } | Self::Forbidden { .. } => None,
        }
    }

    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::Unavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// The `WWW-Authenticate` challenge this rejection sends, if any.
    ///
    /// A 401's is the scheme's
    /// [`challenge`](crate::security::SecurityScheme::challenge), which RFC
    /// 9110 section 11.6.1 requires. A 403's is `None` unless it refused a
    /// scope under a scheme with an
    /// [`insufficient_scope_challenge`](crate::security::SecurityScheme::insufficient_scope_challenge):
    /// RFC 6750 section 3 asks a bearer token's resource server for one when
    /// the token does not enable access, and section 3.1 names
    /// `insufficient_scope` for exactly this 403. Any other 403 sends none,
    /// since repeating an already-valid credential would not change the answer.
    #[must_use]
    pub fn challenge(&self) -> Option<&'static str> {
        match self {
            Self::Unauthenticated { challenge } | Self::Forbidden { challenge, .. } => *challenge,
            // An outage asks nothing of the credential.
            Self::Unavailable { .. } => None,
        }
    }

    /// Sets the challenge a 401 carries, leaving a 403 or a 503 alone.
    ///
    /// Replaces any challenge already set, so the one
    /// [`Auth`](crate::security::auth::Auth) supplies from the scheme is the
    /// one the description declares.
    #[must_use]
    pub fn with_challenge(self, challenge: Option<&'static str>) -> Self {
        match self {
            Self::Unauthenticated { .. } => Self::Unauthenticated { challenge },
            other @ (Self::Forbidden { .. } | Self::Unavailable { .. }) => other,
        }
    }

    /// Sets the challenge a 403 carries, leaving a 401 or a 503 alone.
    ///
    /// [`with_challenge`](Self::with_challenge)'s counterpart for a scope
    /// refusal. Replaces any challenge already set, so a guard's 403 carries
    /// exactly the one it declares, or none.
    #[must_use]
    pub(crate) fn with_scope_challenge(self, challenge: Option<&'static str>) -> Self {
        match self {
            Self::Forbidden { type_uri, .. } => Self::Forbidden {
                type_uri,
                challenge,
            },
            other @ (Self::Unauthenticated { .. } | Self::Unavailable { .. }) => other,
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
                ..
            } => Problem::of_type(status, type_uri, "Forbidden"),
            Self::Forbidden { type_uri: None, .. }
            | Self::Unauthenticated { .. }
            | Self::Unavailable { .. } => Problem::new(status),
        }
        .with_detail(detail)
    }

    fn statuses() -> &'static [StatusCode] {
        &[
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::SERVICE_UNAVAILABLE,
        ]
    }
}

/// Adds the `WWW-Authenticate` field RFC 9110 section 15.5.2 requires on a
/// 401, and RFC 6750 section 3.1 asks of a bearer token's scope refusal, from
/// the challenge the rejection carries; and a 503's `Retry-After`.
impl IntoResponse for AuthRejection {
    fn into_response(self) -> crate::http::Response {
        let challenge = self.challenge();
        let retry_after = self.retry_after();
        let mut response = IntoProblem::into_problem(self).into_response();

        if let Some(delay) = retry_after {
            set_retry_after(&mut response, delay);
        }

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
/// as well, or left wide when `None`, and only where `forbids`,
/// [`SecurityScheme::FORBIDS`] of a scheme the guard names, says one can be
/// sent; and the 503 only where `unavailable`,
/// [`SecurityScheme::MAY_BE_UNAVAILABLE`] of a scheme the guard names, says
/// one can be sent.
pub(crate) fn auth_responses(
    registry: &mut Registry,
    named: Option<&'static str>,
    forbids: bool,
    unavailable: bool,
) -> kynos_openapi::Responses {
    <AuthRejection as IntoProblem>::statuses().iter().fold(
        kynos_openapi::Responses::new(),
        |responses, status| {
            let declared = match *status {
                StatusCode::FORBIDDEN if !forbids => return responses,
                StatusCode::FORBIDDEN => forbidden(registry, *status, named),
                StatusCode::SERVICE_UNAVAILABLE if !unavailable => return responses,
                StatusCode::SERVICE_UNAVAILABLE => narrowed_problem(registry, *status, &[])
                    .with_header("Retry-After", retry_after_header()),
                _ => narrowed_problem(registry, *status, &[]),
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
        return problem_response(registry, crate::http::describe_code(status.as_u16()));
    };

    let problem = registry.resolve::<Problem>();

    narrowed_response(
        &problem,
        status.as_u16(),
        &[(None, None, &[]), (Some(named), None, &[])],
    )
}

/// The rejection a [`Scoped<S, R>`](crate::security::auth::Scoped) argument
/// raises: the same failures [`AuthRejection`] carries — it wraps one —
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
///
/// No 503: the scope set does not know its scheme, so the guard declares that
/// where the scheme opts in. A 403 always, since a scope set is demanded of an
/// authorizer that may refuse it.
impl<R: Scopes> Responses for ScopedRejection<R> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        auth_responses(registry, R::FORBIDDEN_TYPE, true, false)
    }
}

/// The 401 and the 403 left wide: what a hand-written extractor erroring with
/// this type declares.
///
/// A guard declares its own instead, from its schemes: no 403 where none sets
/// [`SecurityScheme::FORBIDS`], and the 503 only where one opts in through
/// [`SecurityScheme::MAY_BE_UNAVAILABLE`]. This type knows no scheme, so it
/// keeps the 403 and declares no 503.
impl Responses for AuthRejection {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        auth_responses(registry, None, true, false)
    }
}

#[cfg(test)]
mod tests;
