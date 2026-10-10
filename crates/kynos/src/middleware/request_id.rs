//! Correlation identifiers.

use std::marker::PhantomData;

use std::{
    convert::Infallible,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    error::rejection::HeaderRejection,
    extract::params::header::{DecodeHeaders, EncodeHeaders, HeaderParams},
    http,
    middleware::{Continued, Interceptor, Next},
    schema::registry::Registry,
};

/// Supplies identifiers for requests that arrive without one.
///
/// Kynos owns the header and the contribution; the identifier *format* stays
/// the application's.
pub trait RequestIdSource: Send + Sync + 'static {
    /// Produces an identifier for a request that carried none.
    fn next_id(&self) -> http::HeaderValue;
}

/// A dependency-free source: a per-process counter.
///
/// Unique within one process and no further; replace it when correlation has
/// to cross a process boundary.
#[derive(Debug, Default)]
pub struct Counter {
    next: AtomicU64,
}

impl RequestIdSource for Counter {
    fn next_id(&self) -> http::HeaderValue {
        // Only uniqueness matters; nothing is ordered against this.
        let id = self.next.fetch_add(1, Ordering::Relaxed);

        http::HeaderValue::from(id)
    }
}

/// A dependency-free source of 128-bit identifiers, written as 32 lowercase
/// hex digits.
///
/// Each identifier is a per-process counter passed through a keyed hash whose
/// key the process draws at random when the source is built. An identifier
/// therefore names one request across restarts and across a fleet, and reveals
/// neither how many requests came before it nor which process minted it.
///
/// Unpredictable only as far as the standard library's keyed hasher is, so it
/// is a correlation handle and never a secret: do not authorize anything by it.
///
/// ```
/// use kynos::middleware::request_id::{Random, RequestId};
///
/// let request_id = RequestId::new().source(Random::new());
/// # let _ = request_id;
/// ```
#[derive(Debug, Default)]
pub struct Random {
    key: std::hash::RandomState,
    next: AtomicU64,
}

impl Random {
    /// A source keyed afresh from the operating system's randomness.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The high and low 64-bit halves of the identifier `n` maps to.
    fn halves(&self, n: u64) -> [u64; 2] {
        use std::hash::BuildHasher;

        // Two halves of one keyed hash, told apart by the second tuple member.
        [self.key.hash_one((n, 0_u8)), self.key.hash_one((n, 1_u8))]
    }
}

impl RequestIdSource for Random {
    fn next_id(&self) -> http::HeaderValue {
        // Only uniqueness matters; nothing is ordered against this.
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let [high, low] = self.halves(n);

        http::HeaderValue::from_str(&format!("{high:016x}{low:016x}"))
            .expect("hex digits are a field value")
    }
}

/// A header group that can carry a correlation identifier.
///
/// [`RequestId`] builds the group it declares from an identifier through
/// this, since an [`Adds`](crate::middleware::Interceptor::Adds) group need
/// not implement `DecodeHeaders`.
///
/// ```
/// use kynos::{
///     extract::params::header::{EncodeHeaders, HeaderParams},
///     http::{HeaderName, HeaderValue},
///     middleware::request_id::CorrelationHeaders,
/// };
///
/// struct TraceId(HeaderValue);
///
/// impl HeaderParams for TraceId {
///     const NAMES: &'static [&'static str] = &["x-trace-id"];
/// }
///
/// // A correlation group only ever *adds* its field, so it writes the
/// // encoding half and never the decoding one.
/// impl EncodeHeaders for TraceId {
///     fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
///         vec![(HeaderName::from_static("x-trace-id"), self.0.clone())]
///     }
/// }
///
/// impl CorrelationHeaders for TraceId {
///     fn from_id(id: HeaderValue) -> Self {
///         Self(id)
///     }
/// }
/// ```
pub trait CorrelationHeaders: EncodeHeaders {
    /// Builds the group from the identifier this request is correlated by.
    ///
    /// One identifier, under every name the group declares.
    fn from_id(id: http::HeaderValue) -> Self;
}

/// The header [`RequestId`] uses unless told otherwise.
///
/// A [`HeaderParams`] group rather than a runtime name, so the description
/// can print it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XRequestId(
    /// The identifier carried by this request.
    pub http::HeaderValue,
);

impl HeaderParams for XRequestId {
    const NAMES: &'static [&'static str] = &["x-request-id"];

    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        vec![
            kynos_openapi::Parameter::header("X-Request-Id", identifier_schema())
                .required(true)
                .with_description("The identifier this request is correlated by"),
        ]
    }

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;
        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "X-Request-Id".to_owned(),
            kynos_openapi::RefOr::Item(
                kynos_openapi::Header::new(identifier_schema())
                    .with_description("The identifier this request is correlated by"),
            ),
        );
        headers
    }
}

impl DecodeHeaders for XRequestId {
    fn decode(headers: &http::HeaderMap) -> Result<Self, HeaderRejection> {
        headers
            .get("x-request-id")
            .cloned()
            .map(Self)
            .ok_or_else(|| HeaderRejection::Invalid {
                name: "X-Request-Id".to_owned(),
                detail: "the header is absent".to_owned(),
            })
    }
}

impl EncodeHeaders for XRequestId {
    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        vec![(
            http::HeaderName::from_static("x-request-id"),
            self.0.clone(),
        )]
    }
}

impl CorrelationHeaders for XRequestId {
    fn from_id(id: http::HeaderValue) -> Self {
        Self(id)
    }
}

/// The schema of an identifier: a string, since a replaced
/// [`RequestIdSource`] chooses the format.
fn identifier_schema() -> kynos_openapi::Schema {
    kynos_openapi::Schema::of_type(kynos_openapi::model::schema::types::SchemaType::String)
}

/// Assigns each request an identifier and echoes it back.
///
/// The header group `H` is both what every covered operation documents and
/// what the response carries.
pub struct RequestId<S = Counter, H = XRequestId> {
    source: S,
    // Read by `Trace`, which logs an inbound identifier only where this echoes it.
    pub(super) trust_client: bool,
    _header: PhantomData<fn() -> H>,
}

// Hand-written so `H` needs no `Clone` bound.
impl<S: Clone, H> Clone for RequestId<S, H> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            trust_client: self.trust_client,
            _header: PhantomData,
        }
    }
}

impl<S: std::fmt::Debug, H: HeaderParams> std::fmt::Debug for RequestId<S, H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestId")
            .field("source", &self.source)
            .field("header", &H::NAMES)
            .field("trust_client", &self.trust_client)
            .finish_non_exhaustive()
    }
}

impl Default for RequestId<Counter, XRequestId> {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestId<Counter, XRequestId> {
    /// Uses `X-Request-Id`, generating one when the client sends none.
    #[must_use]
    pub fn new() -> Self {
        Self {
            source: Counter::default(),
            trust_client: false,
            _header: PhantomData,
        }
    }
}

impl<S: RequestIdSource, H: HeaderParams> RequestId<S, H> {
    /// Uses a different header group.
    ///
    /// Changing the group changes what every covered operation declares. A
    /// group naming more than one header sets and documents all of them.
    ///
    /// The group must implement [`CorrelationHeaders`], which
    /// `#[derive(HeaderParams)]` does not supply.
    #[must_use]
    pub fn header<G: CorrelationHeaders>(self) -> RequestId<S, G> {
        RequestId {
            source: self.source,
            trust_client: self.trust_client,
            _header: PhantomData,
        }
    }

    /// Echoes a client-supplied identifier instead of always generating one.
    ///
    /// Off by default, since an inbound header is attacker-controlled.
    #[must_use]
    pub fn trust_client(mut self, trust: bool) -> Self {
        self.trust_client = trust;
        self
    }

    /// Replaces the identifier source.
    #[must_use]
    pub fn source<T: RequestIdSource>(self, source: T) -> RequestId<T, H> {
        RequestId {
            source,
            trust_client: self.trust_client,
            _header: PhantomData,
        }
    }
}

impl<C, S, H> Interceptor<C> for RequestId<S, H>
where
    C: Sync + 'static,
    S: RequestIdSource,
    H: CorrelationHeaders + Send + Sync + 'static,
{
    type Reads = ();
    type Adds = H;

    /// Always continues: an identifier is added to whatever the chain returns.
    type Short = Infallible;

    async fn intercept(
        &self,
        mut request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<H>, Infallible> {
        let _ = (reads, context);

        // The first declared name wins: one identifier is carried under all.
        let inbound = if self.trust_client {
            H::NAMES
                .iter()
                .find_map(|name| request.headers().get(*name).cloned())
        } else {
            None
        };

        let id = inbound.unwrap_or_else(|| self.source.next_id());

        let headers = H::from_id(id);

        // Set on the request too, so handler, observer and client correlate on
        // one value under the same names.
        for (name, value) in headers.encode() {
            request.headers_mut().insert(name, value);
        }

        Ok(next.run(request).await.with_headers(headers))
    }
}

#[cfg(test)]
mod tests;
