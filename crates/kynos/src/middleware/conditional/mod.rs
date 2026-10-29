//! Answering a request whose copy is already current.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    error::rejection::HeaderRejection,
    extract::params::header::{DecodeHeaders, HeaderParams},
    http::{self, HeaderMap, HeaderValue, etag, header},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// What the client already holds: a request's `If-None-Match` field lines.
///
/// The lines are kept as sent and read only where they are compared, so a
/// request pays for no copy of its tags. Only lines of visible ASCII are kept,
/// since no other line can name a tag. Two are equal when they hold the same
/// lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfNoneMatch {
    // Split from the rest so the one line a request almost always sends needs
    // no buffer.
    first: HeaderValue,
    rest: Vec<HeaderValue>,
}

impl IfNoneMatch {
    /// The entity tags the client holds, verbatim, or `None` where it sent
    /// `*`, which names any representation the server has.
    ///
    /// Every field line is read as one list (RFC 9110 section 5.3); a line
    /// that is `*` makes the whole condition `*`.
    #[must_use]
    pub fn tags(&self) -> Option<impl Iterator<Item = &str>> {
        if self.texts().any(|text| etag::listed(text).is_none()) {
            return None;
        }

        Some(self.texts().flat_map(etag::split))
    }

    /// The field lines, in the order they were sent.
    fn lines(&self) -> impl Iterator<Item = &HeaderValue> {
        core::iter::once(&self.first).chain(&self.rest)
    }

    /// The field lines as text, which every kept line is.
    fn texts(&self) -> impl Iterator<Item = &str> {
        self.lines().filter_map(|line| line.to_str().ok())
    }
}

/// The preconditions [`Conditional`] evaluates.
///
/// `If-Match` and `If-Unmodified-Since` are absent: they must be evaluated
/// before the change, which only the handler can do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preconditions {
    /// What the client already holds, if it said.
    pub if_none_match: Option<IfNoneMatch>,
}

impl HeaderParams for Preconditions {
    const NAMES: &'static [&'static str] = &["if-none-match"];

    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;

        vec![
            kynos_openapi::Parameter::header(
                "If-None-Match",
                kynos_openapi::Schema::of_type(SchemaType::String),
            )
            .with_description(
                "The entity tags the client already holds, per RFC 9110 section 13.1.2. A match \
                 is answered with 304.",
            ),
        ]
    }
}

impl DecodeHeaders for Preconditions {
    const FALLIBLE: bool = false;

    /// Never fails.
    ///
    /// RFC 9110 section 13.1: a recipient ignores a condition it cannot
    /// evaluate, so a malformed precondition is absent rather than a 400. A
    /// field none of whose lines is visible ASCII is absent.
    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        // A field value shares the request's buffer, so a clone copies no
        // bytes.
        let mut lines = headers
            .get_all(header::IF_NONE_MATCH)
            .iter()
            .filter(|line| line.to_str().is_ok())
            .cloned();

        let if_none_match = lines.next().map(|first| IfNoneMatch {
            first,
            rest: lines.collect(),
        });

        Ok(Self { if_none_match })
    }
}

/// The 304 a matched precondition produces.
#[derive(Clone, Debug, Default)]
pub struct NotModified {
    replayed: HeaderMap,
}

impl NotModified {
    /// The 304 for a response carrying `headers`.
    ///
    /// RFC 9110 section 15.4.5 requires a 304 to carry the fields a 200 would
    /// have sent that a cache needs to update its stored copy, plus
    /// `Last-Modified` for a client that validated with a date.
    #[must_use]
    pub fn from_headers(headers: &HeaderMap) -> Self {
        const REPLAYED: &[http::HeaderName] = &[
            header::CACHE_CONTROL,
            header::CONTENT_LOCATION,
            header::DATE,
            header::ETAG,
            header::EXPIRES,
            header::VARY,
            header::AGE,
            header::LAST_MODIFIED,
        ];

        let mut replayed = HeaderMap::new();
        for name in REPLAYED {
            for value in headers.get_all(name) {
                replayed.append(name.clone(), value.clone());
            }
        }

        Self { replayed }
    }
}

impl IntoResponse for NotModified {
    fn into_response(self) -> http::Response {
        let mut response = crate::http::body::bodiless(http::StatusCode::NOT_MODIFIED);
        *response.headers_mut() = self.replayed;
        response
    }
}

impl ShortCircuit for NotModified {
    const STATUSES: &'static [u16] = &[304];
}

impl Responses for NotModified {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;

        kynos_openapi::Responses::new().with(
            304,
            kynos_openapi::Response::new("the client's copy is current").with_header(
                "ETag",
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
                    .with_description("The entity tag the precondition matched"),
            ),
        )
    }
}

/// Answers a safe request with 304 when the client's copy is current.
///
/// Contributes 304 and declares `If-None-Match`.
///
/// # Why it runs the handler first
///
/// RFC 9110 section 13 evaluates a precondition against the *current*
/// representation, and only the handler knows what that is. A
/// [`Continued`] deliberately cannot change a
/// status, so the 304 is a short circuit taken after the chain returns rather
/// than before it runs.
///
/// The handler's work is therefore done and discarded, which is why this
/// belongs *outside* a [`Cache`](crate::middleware::cache::Cache) (the earlier
/// `intercept` call, per
/// [the module's ordering rule](crate::middleware#the-order-a-chain-runs-in)):
/// a cache hit never reaches a `Conditional` mounted inside it.
///
/// Safe methods only: `If-None-Match` on an unsafe method calls for a 412, not
/// a 304.
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditional;

impl Conditional {
    /// Answers a matched precondition with 304.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl<C: Sync + 'static> Interceptor<C> for Conditional {
    type Reads = Preconditions;
    type Adds = ();
    type Short = NotModified;

    async fn intercept(
        &self,
        request: http::Request,
        reads: Preconditions,
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, NotModified> {
        let _ = context;

        let safe = matches!(request.method(), &http::Method::GET | &http::Method::HEAD);
        let continued = next.run(request).await;

        let Some(condition) = reads.if_none_match.filter(|_| safe) else {
            return Ok(continued);
        };

        // Only a 200, not any 2xx: RFC 9110 section 15.4.5 defines 304 as the
        // answer to a request that "would have resulted in a 200 (OK) response".
        if continued.status() != http::StatusCode::OK {
            return Ok(continued);
        }

        if matched(&condition, continued.headers()) {
            return Err(NotModified::from_headers(continued.headers()));
        }

        Ok(continued)
    }
}

/// Whether the client's copy is the one the response carries.
///
/// [`etag::if_none_match_lines`], save that a response carrying no tag matches
/// nothing, `*` included, which that would match: with no validator to
/// compare, a 304 would call the client's copy current on no evidence.
fn matched(condition: &IfNoneMatch, headers: &HeaderMap) -> bool {
    let Some(current) = headers
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };

    etag::if_none_match_lines(condition.lines(), || Some(current)) == Some(true)
}

#[cfg(test)]
mod tests;
