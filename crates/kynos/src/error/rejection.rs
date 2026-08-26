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

use std::collections::BTreeMap;

use serde_json::json;

use kynos_openapi::model::body::mime_names::APPLICATION_PROBLEM_JSON;

use crate::{
    error::problem::{IntoProblem, Problem},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// One response per declared status, each an `application/problem+json`
/// document referring to the shared [`Problem`] component.
fn problem_responses(registry: &mut Registry, statuses: &[StatusCode]) -> kynos_openapi::Responses {
    let schema = registry.resolve::<Problem>();

    statuses
        .iter()
        .fold(kynos_openapi::Responses::new(), |responses, status| {
            let description = status.canonical_reason().map_or_else(
                || format!("a `{}` response", status.as_u16()),
                str::to_owned,
            );

            responses.with(
                status.as_u16(),
                kynos_openapi::Response::with_content(
                    description,
                    APPLICATION_PROBLEM_JSON,
                    kynos_openapi::MediaType::new(schema.clone()),
                ),
            )
        })
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
    Forbidden,
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

    /// The status this rejection produces.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated { .. } => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
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
            Self::Forbidden => None,
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
            Self::Forbidden => Self::Forbidden,
        }
    }
}

impl IntoProblem for AuthRejection {
    fn into_problem(self) -> Problem {
        // Nothing beyond the sentence the variant already carries: which check
        // refused the request is exactly what an attacker would like to learn,
        // and a client can act on neither answer differently. The challenge is
        // not part of it -- RFC 9110 puts that in a header, not in a body.
        Problem::new(self.status()).with_detail(self.to_string())
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

impl Responses for AuthRejection {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        problem_responses(registry, <AuthRejection as IntoProblem>::statuses())
    }
}

#[cfg(test)]
mod tests;
