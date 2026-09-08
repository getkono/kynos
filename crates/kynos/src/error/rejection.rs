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
//! 408 — are not here. [`RateLimit`](crate::middleware::rate_limit::RateLimit),
//! [`Concurrency`](crate::middleware::limits::Concurrency) and
//! [`Timeout`](crate::middleware::limits::Timeout) return a response directly
//! and declare it through `OperationContribution`.
//!
//! # What a rejection says
//!
//! Everything a rejection carries reaches the client, so a variant holds only
//! what the request itself already determined: which parameter, which media
//! type, the limit it exceeded, where in the body a value went wrong. Nothing
//! here names server state, and an authentication failure says only that it
//! failed — RFC 9457 §5 is explicit that a problem is not a debugging channel,
//! and which of several credential checks refused a request is the server's
//! business.
//!
//! One field is not that shape, and is an exception rather than a hole in the
//! rule: the problem type on [`AuthRejection::Forbidden`]. The request did not
//! determine it and neither did Kynos — the *application's* authorizer supplies
//! it, because only the application knows which of its authorization rules
//! declined. It is still a fact the caller may have: it names a class of
//! refusal, not the check that produced one.

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
/// component to the type URI a rejection publishes.
///
/// Every rejection *this* function describes builds its problem with
/// [`Problem::new`], so every one of them writes `about:blank` — which makes
/// the narrowing a true statement rather than a guess, and states in the
/// document what a client otherwise has to learn by receiving one. The
/// qualification is not idle: [`AuthRejection`] below reaches
/// [`Problem::of_type`] for a 403 an authorizer named, which is why that one
/// status does not come through here.
///
/// The narrowing is also what lets a rejection meet a handler's error type on a
/// status without either of them losing. Two narrowed problem responses union
/// into a choice between the types each publishes; a bare `$ref` beside one
/// would match every problem document and cost that choice its exactly-one
/// rule, so it wins outright instead and the derive's narrowing is discarded.
/// [`OperationCx::add_responses`](crate::router::operation::OperationCx::add_responses)
/// is where the two meet.
///
/// The one status this cannot describe is [`AuthRejection`]'s 403, which
/// declares itself below.
///
/// A rejection declaring no status registers no `Problem` component either,
/// which is right: it declares no problem.
fn problem_responses(registry: &mut Registry, statuses: &[StatusCode]) -> kynos_openapi::Responses {
    statuses
        .iter()
        .fold(kynos_openapi::Responses::new(), |responses, status| {
            responses.with(status.as_u16(), narrowed_problem(registry, *status))
        })
}

/// The response one status declares: the shared component narrowed to
/// `about:blank`.
///
/// One branch naming no URI, which is what every rejection but one publishes,
/// and no summary, because the status code's reason phrase is all a rejection
/// has to say about itself.
fn narrowed_problem(registry: &mut Registry, status: StatusCode) -> kynos_openapi::Response {
    let problem = registry.resolve::<Problem>();

    narrowed_response(&problem, status.as_u16(), &[(None, None)])
}

/// Emits the two implementations that are mechanical for every rejection: the
/// bridge to a response, and the description built from `statuses()`.
///
/// Hand-writing fourteen identical bodies would invite one of them to drift.
/// `into_problem` stays per-type, because only it knows the variants.
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
///
/// The one rejection with a genuinely wide status set, because deciding a body
/// is unacceptable happens in three distinct ways.
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
        let problem = Problem::new(self.status());
        let summary = self.to_string();

        match self {
            Self::Syntax { detail } => problem.with_detail(format!("{summary}: {detail}")),

            // A set of failures cannot fit in one sentence, so it travels as
            // RFC 9457's `errors` extension: one entry per pointer, which is
            // the shape the specification's own validation example uses.
            Self::Schema { failures } => {
                let errors: Vec<_> = failures
                    .into_iter()
                    .map(|(pointer, detail)| json!({ "pointer": pointer, "detail": detail }))
                    .collect();

                problem
                    .with_detail(summary)
                    .with_extension("errors", errors)
            }

            Self::UnsupportedMediaType { received } => problem.with_detail(received.map_or_else(
                || format!("{summary}: the request declared no `Content-Type`"),
                |received| format!("{summary}: `{received}`"),
            )),
        }
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
/// The only status a `Range` field can produce that is not a success. Every
/// *other* way a `Range` can be unusable — an unknown unit, a malformed value,
/// a method for which range handling is not defined — is one RFC 9110 section
/// 14.2 answers by ignoring the field, so
/// [`Range<T>`](crate::response::range::Range) is an infallible extractor and
/// this is raised by [`Range::apply`](crate::response::range::Range::apply)
/// rather than while the request head is read.
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum RangeRejection {
    /// The field was understood and no spec in it is satisfiable. Produces 416.
    #[error("no requested range is satisfiable")]
    NotSatisfiable {
        /// The length of the selected representation.
        ///
        /// Carried on the variant because a rejection holds what the request
        /// already determined, and because this is the number that tells a
        /// client which range to ask for instead. RFC 9110 section 15.5.17 asks
        /// a 416 to state it.
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
        // The complete length is not repeated in the document: RFC 9110 puts it
        // in `Content-Range`, and a second spelling in the body would be a
        // number a client could find disagreeing with the field it is told to
        // read.
        Problem::new(self.status()).with_detail(self.to_string())
    }

    fn statuses() -> &'static [StatusCode] {
        &[StatusCode::RANGE_NOT_SATISFIABLE]
    }
}

/// One of the two rejections whose response is more than a problem document.
///
/// RFC 9110 section 15.5.17: *a server that generates a 416 response to a
/// byte-range request SHOULD generate a Content-Range header field specifying
/// the current length of the selected representation.* `Problem::into_response`
/// sets no header, so `rejection_response!` cannot produce this one.
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
/// Declared here rather than by the argument that reads the `Range`, which is
/// where [`AuthRejection`] and `Auth<S>` differ: reading a credential can fail,
/// so a 401 belongs to the extractor, and only the scheme knows the challenge
/// string, so only `Auth::describe` can supply it. Reading a `Range` cannot
/// fail. The 416 originates in [`Range::apply`](crate::response::range::Range::apply),
/// so it reaches the document through the handler's return type — declared
/// exactly on the operations that can produce it, and never on one that reads
/// the field and answers whole.
///
/// The header rides along because its shape is fixed. There is one
/// `unsatisfied-range` grammar and no per-operation string to fill in, so the
/// rejection can describe the response it sends without help.
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
        /// `None` — use [`AuthRejection::unauthenticated`] — because the
        /// challenge belongs to the scheme rather than to the check.
        /// [`Auth`](crate::security::auth::Auth) fills it in from
        /// [`SecurityScheme::challenge`](crate::security::SecurityScheme::challenge)
        /// on the way out, which is what makes the string on the wire and the
        /// one the operation declares the same string.
        challenge: Option<&'static str>,
    },

    /// Credentials were valid but insufficient. Produces 403.
    #[error("access is not permitted")]
    Forbidden {
        /// The problem `type` this 403 carries, or `about:blank` when `None`.
        ///
        /// The asymmetry with `Unauthenticated` is the point. A 401 has no such
        /// field: which credential check refused is the server's business, and
        /// a client can act on neither answer differently. An *authorization*
        /// rule is the opposite — "this account is suspended" is a class of
        /// refusal a client acts on, and only the application knows which of
        /// its rules refused, so the URI is a value the rejection carries
        /// rather than one Kynos could name for it.
        ///
        /// Set it with [`AuthRejection::forbidden_as`] and leave it unset with
        /// [`AuthRejection::forbidden`]. `&'static str` rather than an owned
        /// string is where the friction belongs, not a micro-optimisation: the
        /// URI reaches the client verbatim and is not validated, so a name
        /// assembled from the request has to be leaked before it can be passed
        /// — which is deliberate enough that nobody does it by accident, and
        /// is the whole of the guarantee. It is the same bound
        /// `Unauthenticated`'s `challenge` carries one variant above.
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
    /// the refusal has no name of its own — the 403 every version of this enum
    /// has produced.
    #[must_use]
    pub const fn forbidden() -> Self {
        Self::Forbidden { type_uri: None }
    }

    /// A 403 carrying the problem type the authorizer chose for it.
    ///
    /// The one part of a rejection Kynos cannot supply: a refusal means
    /// something in the application's own vocabulary, and RFC 9457 section
    /// 3.1.1 is where that meaning is spelled. The title stays the status
    /// code's reason phrase — section 3.1.3 makes a title a property of the
    /// type, and Kynos has none to offer for a URI it has never seen; an
    /// application wanting its own title has `#[derive(ApiError)]`.
    ///
    /// **Whether the description says so is the scope set's to decide.** A
    /// [`Scoped<S, R>`](crate::security::auth::Scoped) argument whose `R` names
    /// a
    /// [`FORBIDDEN_TYPE`](crate::security::auth::Scopes::FORBIDDEN_TYPE)
    /// declares a 403 publishing that URI or `about:blank`, and passing the
    /// same URI here is what makes the two agree. Anywhere else — an `R` naming
    /// none, or an [`Auth<S>`](crate::security::auth::Auth) or
    /// [`MaybeAuth<S>`](crate::security::auth::MaybeAuth) argument, which names
    /// no scope set to hang a const on — the declared 403 stays the shared
    /// `Problem` component, and a client reading the body sees a URI a client
    /// reading the description does not.
    ///
    /// The URI is not validated and reaches the client verbatim, so it names a
    /// class of refusal rather than a fact about the caller. RFC 9457 section
    /// 3.1.1 recommends an absolute URI: a relative reference resolves against
    /// the request, so the same refusal would carry a different identity per
    /// endpoint. `&'static str` is
    /// what holds an author to that: a URI assembled from the request would
    /// have to be leaked to be passed here, which is friction in exactly the
    /// right place. A `const` refusal is the ordinary case.
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
    /// Replaces rather than fills a gap. [`Auth`](crate::security::auth::Auth)
    /// calls this with the scheme's own challenge, and that is the one the
    /// operation's description declares; an authenticator that supplied a
    /// different one would make the document wrong about what a client
    /// receives.
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
        // Nothing Kynos adds beyond the sentence the variant already carries:
        // which check refused the request is exactly what an attacker would
        // like to learn, and a client can act on neither answer differently.
        // The challenge is not part of it -- RFC 9110 puts that in a header,
        // not in a body. The one thing that does reach the document came from
        // the application, below.
        let status = self.status();
        let detail = self.to_string();

        match self {
            // The exception, and the only one: an authorizer that named its
            // refusal named something Kynos does not know. The title is the
            // reason phrase `Problem::new` would have given it, for the reason
            // `forbidden_as` states.
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

/// The other rejection whose response is more than a problem document.
///
/// RFC 9110 section 15.5.2: a server generating a 401 MUST send a
/// `WWW-Authenticate` header field. Only the scheme knows the challenge, so it
/// rides on the rejection rather than being reconstructed here — which is also
/// what keeps it identical to the one
/// [`Auth`](crate::security::auth::Auth)'s description declares.
impl IntoResponse for AuthRejection {
    fn into_response(self) -> crate::http::Response {
        let challenge = self.challenge();
        let mut response = IntoProblem::into_problem(self).into_response();

        // `from_str` rather than `from_static`: a challenge is an ordinary
        // `&'static str` a `SecurityScheme` implementation supplies, and one
        // carrying a newline would splice a header of its choosing into the
        // response. An unrepresentable challenge is dropped, because a response
        // path that panics is worse than a 401 missing an advisory header --
        // and `Auth`'s description withholds the header on the same condition,
        // so the two still agree.
        if let Some(value) = challenge.and_then(|challenge| HeaderValue::from_str(challenge).ok()) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }

        response
    }
}

/// The one rejection whose statuses are not described alike, given the type its
/// 403 may name.
///
/// The 401 narrows like every other rejection: [`AuthRejection::unauthenticated`]
/// leaves the type to [`Problem::new`], and which credential check refused is
/// not a fact this type will ever carry.
///
/// The 403 is the one status in the crate an *application* decides the type of.
/// [`AuthRejection::forbidden_as`] takes a URI at run time, while a description
/// is assembled from types — so `named` is the only thing about it a type can
/// say, and it comes from
/// [`Scopes::FORBIDDEN_TYPE`](crate::security::auth::Scopes::FORBIDDEN_TYPE) on
/// the scope set a [`Scoped<S, R>`](crate::security::auth::Scoped) argument
/// demanded.
///
/// Driven from [`IntoProblem::statuses`] rather than from two literals, so a
/// status added there is described rather than silently dropped.
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
/// **A choice, never the named URI alone.** [`AuthRejection::forbidden`] stays
/// available to every authorizer whatever its scope set declares, so both
/// bodies are observable on the operation and a narrowing to either one would
/// declare less than it sends — the direction `emitted ⊇ observable` forbids.
/// `about:blank` leads, because it is the answer a refusal with nothing of its
/// own to say has always given.
///
/// Naming none leaves the shared component, which is the *widest* thing that
/// can be declared rather than a gap: an authorizer that names no URI here may
/// still name any URI at all on the wire, and only a schema every problem
/// document satisfies is true of that. It is what every guard declared before
/// [`Scopes::FORBIDDEN_TYPE`](crate::security::auth::Scopes::FORBIDDEN_TYPE)
/// existed, and what [`Auth<S>`](crate::security::auth::Auth) and
/// [`MaybeAuth<S>`](crate::security::auth::MaybeAuth) still declare.
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
/// # Why the guard's rejection is not `AuthRejection` itself
///
/// Because `Responses` is reached through the *rejection type*, and a 403
/// narrowed anywhere else is overwritten by the wide one `AuthRejection` would
/// contribute beside it. A side admitting every problem document wins a union
/// outright — the rule that keeps [`Auth<S>`](crate::security::auth::Auth)'s
/// 403 sound, since an authorizer there may name a URI no type can — and it
/// applies whichever contributor carries it. So a scope set that names its
/// refusal has to be read by the contributor that decides the status, which is
/// this type.
///
/// [`Scopes::FORBIDDEN_TYPE`] is what it reads, and naming none declares
/// exactly what `AuthRejection` does.
///
/// Nothing constructs one but the guard, and an
/// [`Authenticator`](crate::security::Authenticator) still returns an
/// `AuthRejection`: what the check knows is which failure occurred, and the
/// scope set is what the *argument* knows.
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

/// Hand-written rather than derived, on the rule
/// [`Auth`](crate::security::auth::Auth) states one file over: a derive would
/// bound the implementation on the scope set, which is a marker carrying
/// nothing, while what is actually being formatted is the rejection.
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

/// No `source`, deliberately: `Display` above is the wrapped rejection's own
/// sentence, so a chain entry beneath it would print that sentence twice. It is
/// the rule [`errors.md`] states for a value whose `Display` is self-contained.
///
/// [`errors.md`]: https://github.com/getkono/kynos/blob/master/docs/errors.md
impl<R: Scopes> std::error::Error for ScopedRejection<R> {}

impl<R: Scopes> From<AuthRejection> for ScopedRejection<R> {
    fn from(rejection: AuthRejection) -> Self {
        Self::new(rejection)
    }
}

/// Byte for byte what the wrapped rejection writes: the scope set is a fact
/// about the *description*, and nothing about it reaches a client.
impl<R: Scopes> IntoResponse for ScopedRejection<R> {
    fn into_response(self) -> crate::http::Response {
        self.rejection.into_response()
    }
}

/// The 401 and the 403, the second narrowed to what
/// [`Scopes::FORBIDDEN_TYPE`] names — which is the whole reason this type is
/// not `AuthRejection`.
impl<R: Scopes> Responses for ScopedRejection<R> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        auth_responses(registry, R::FORBIDDEN_TYPE)
    }
}

/// What a guard naming no scope set declares, which is what every guard
/// declared before one could.
///
/// The trait method takes a registry and nothing else, so this is the only
/// answer it has: a `Responses` implementation is reached from the rejection
/// type, and the type carries no scope set. `auth_responses` above is where the
/// named case is reached from, and `security::auth::declare` is its only caller.
impl Responses for AuthRejection {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        auth_responses(registry, None)
    }
}

#[cfg(test)]
mod tests;
