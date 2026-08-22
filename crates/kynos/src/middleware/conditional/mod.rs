//! Answering a request whose copy is already current.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    error::rejection::HeaderRejection,
    extract::params::header::HeaderParams,
    http::{self, HeaderMap, HeaderValue, etag, header},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// An entity tag a handler attaches to its own response.
///
/// A [`HeaderParams`] group, so attaching one is *declaring* one and the
/// conflict check sees it. Return it through
/// [`WithHeaders`](crate::response::headers::WithHeaders).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ETag {
    /// The tag, without quotes or a weakness marker.
    pub value: String,
    /// Whether the tag is weak.
    pub weak: bool,
}

impl ETag {
    /// A strong tag: the representation is byte-for-byte this one.
    #[must_use]
    pub fn strong(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weak: false,
        }
    }

    /// A weak tag: the representation is equivalent, not identical.
    #[must_use]
    pub fn weak(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weak: true,
        }
    }

    /// The field value.
    #[must_use]
    pub fn encode(&self) -> Option<HeaderValue> {
        // RFC 9110 section 8.8.3: `etagc` is printable ASCII without `"`.
        if !self
            .value
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte) && byte != b'"')
        {
            return None;
        }

        let marker = if self.weak { "W/" } else { "" };
        HeaderValue::from_str(&format!("{marker}\"{}\"", self.value)).ok()
    }
}

impl HeaderParams for ETag {
    const NAMES: &'static [&'static str] = &["etag"];

    fn encode(&self) -> Vec<(http::HeaderName, HeaderValue)> {
        Self::encode(self)
            .map(|value| vec![(header::ETAG, value)])
            .unwrap_or_default()
    }

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "ETag".to_owned(),
            kynos_openapi::RefOr::Item(
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
                    .with_description("The entity tag of this representation"),
            ),
        );
        headers
    }
}

/// What the client already holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IfNoneMatch {
    /// `*`: any representation the server has.
    Any,
    /// The entity tags the client holds, verbatim.
    Tags(Vec<String>),
}

/// The preconditions [`Conditional`] evaluates.
///
/// `If-Match` and `If-Unmodified-Since` are deliberately absent. Honouring
/// either on an unsafe method means evaluating it *before* the change, which
/// only the handler can do — and an interceptor claiming a 412 it decided
/// afterwards would be advertising lost-update protection it does not provide.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preconditions {
    /// What the client already holds, if it said.
    pub if_none_match: Option<IfNoneMatch>,
}

impl HeaderParams for Preconditions {
    const NAMES: &'static [&'static str] = &["if-none-match"];

    /// Never fails.
    ///
    /// RFC 9110 section 13.1: a recipient ignores a condition it cannot
    /// evaluate. So a malformed precondition is *absent* rather than a 400, and
    /// this interceptor adds no rejection to the operations it covers.
    fn decode(headers: &HeaderMap) -> Result<Self, HeaderRejection> {
        Ok(Self {
            if_none_match: headers
                .get(header::IF_NONE_MATCH)
                .and_then(|value| value.to_str().ok())
                .map(|text| {
                    if text.trim() == etag::ANY {
                        IfNoneMatch::Any
                    } else {
                        IfNoneMatch::Tags(etag::split(text).map(str::to_owned).collect())
                    }
                }),
        })
    }

    fn encode(&self) -> Vec<(http::HeaderName, HeaderValue)> {
        Vec::new()
    }

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

/// The 304 a matched precondition produces.
#[derive(Clone, Debug, Default)]
pub struct NotModified {
    replayed: HeaderMap,
}

impl NotModified {
    /// The 304 for a response carrying `headers`.
    ///
    /// RFC 9110 section 15.4.5 requires a 304 to carry the fields a 200 would
    /// have sent that a cache needs to update its stored copy. `Last-Modified`
    /// is absent from that list and included anyway, because a client that
    /// validated with a date has nothing else to refresh its record with —
    /// though Kynos never sends one itself.
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
        let mut response = http::Response::new(crate::http::body::Body::empty());
        *response.status_mut() = http::StatusCode::NOT_MODIFIED;
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
/// The handler's work is therefore done and discarded. That is the cost, and it
/// is why mounting this *outside* a [`Cache`](crate::middleware::cache::Cache)
/// matters: a cache hit is cheap, and turning a cheap hit into a 304 is the
/// arrangement worth having.
///
/// Safe methods only. `If-None-Match` on an unsafe method means something else
/// entirely — "only if it does not already exist" — and answering that with a
/// 304 rather than a 412 would be wrong in the direction that loses data.
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

        // Only a success can be revalidated. A 404 that matched a stale tag is
        // still a 404.
        if !continued.status().is_success() {
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
/// The *weak* comparison, per RFC 9110 section 13.1.2: `W/"x"` and `"x"` are
/// the same representation for a cache validation, which is what
/// `If-None-Match` is for.
fn matched(condition: &IfNoneMatch, headers: &HeaderMap) -> bool {
    let Some(current) = headers
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
    else {
        // No validator, nothing to match. `*` included: RFC 9110 says it
        // matches "any current representation", and a response carrying no tag
        // has none to compare.
        return false;
    };

    match condition {
        IfNoneMatch::Any => true,
        IfNoneMatch::Tags(tags) => tags.iter().any(|tag| etag::weak_match(tag, current)),
    }
}

#[cfg(test)]
mod tests;
