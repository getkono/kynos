//! Correlation identifiers.

use std::marker::PhantomData;

use std::{
    convert::Infallible,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    error::rejection::HeaderRejection,
    extract::params::header::{DecodeHeaders, EncodeHeaders, HeaderParams, UnencodableHeader},
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

        Digits::<20>::value::<10>(id.into(), 1)
    }
}

/// An identifier's digits, held inline so the value carrying them is one
/// allocation every clone shares: `http`'s own constructors allocate twice,
/// or once and again on the first clone, and [`RequestId`] clones the value
/// it sets on the request onto the response.
struct Digits<const N: usize> {
    buf: [u8; N],
    start: usize,
}

impl<const N: usize> AsRef<[u8]> for Digits<N> {
    fn as_ref(&self) -> &[u8] {
        &self.buf[self.start..]
    }
}

impl<const N: usize> Digits<N> {
    /// `n` in `RADIX`, zero-padded to `width` digits.
    fn value<const RADIX: u128>(mut n: u128, width: usize) -> http::HeaderValue {
        let mut digits = Self {
            buf: [0; N],
            start: N,
        };
        while digits.start > N - width || n > 0 {
            digits.start -= 1;
            digits.buf[digits.start] =
                b"0123456789abcdef"[usize::try_from(n % RADIX).expect("a digit")];
            n /= RADIX;
        }

        http::HeaderValue::from_maybe_shared(bytes::Bytes::from_owner(digits))
            .expect("digits are a field value")
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

        Digits::<32>::value::<16>(u128::from(high) << 64 | u128::from(low), 32)
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
///     extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
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
///     fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
///         Ok(vec![(HeaderName::from_static("x-trace-id"), self.0.clone())])
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
    fn encode(&self) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, UnencodableHeader> {
        Ok(vec![(
            http::HeaderName::from_static("x-request-id"),
            self.0.clone(),
        )])
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

/// The identifier a request is correlated by, as [`RequestId`] assigned it.
///
/// `RequestId` inserts it into the request's extensions before the rest of the
/// chain runs, and into the response's on the way out, so whatever reads it
/// names no header: it is the same value under whichever header group the
/// router's `RequestId` was given. It is absent where no `RequestId` covers the
/// operation or reaches it, as for a request an earlier interceptor answers,
/// and where the group refused to encode it, since that response carries no
/// identifier either.
///
/// A handler takes it as `Option<AssignedId>`, which never rejects and
/// describes nothing: the header is `RequestId`'s to describe.
///
/// ```
/// use kynos::{
///     Router,
///     extract::body::text::Text,
///     middleware::request_id::{AssignedId, RequestId},
/// };
///
/// #[kynos::get("/whoami")]
/// async fn whoami(id: Option<AssignedId>) -> Text {
///     Text(id.as_ref().and_then(AssignedId::as_str).unwrap_or_default().to_owned())
/// }
///
/// let router = Router::<()>::new()
///     .mount(kynos::routes![whoami])
///     .intercept(RequestId::new());
/// # let _ = router;
/// ```
///
/// An interceptor mounted inside `RequestId` reads it with
/// [`of`](AssignedId::of), and an [`Observer`](crate::middleware::Observer)
/// with [`of_response`](AssignedId::of_response).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignedId(http::HeaderValue);

impl AssignedId {
    /// The identifier `RequestId` assigned `request`, if one did.
    #[must_use]
    pub fn of(request: &http::Request) -> Option<&Self> {
        request.extensions().get()
    }

    /// The identifier `RequestId` wrote on `response`, if one did.
    #[must_use]
    pub fn of_response(response: &http::Response) -> Option<&Self> {
        response.extensions().get()
    }

    /// The identifier as text, or `None` where it is not visible ASCII, which
    /// a trusted client or a replaced [`RequestIdSource`] may send.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.0.to_str().ok()
    }

    /// The identifier as the field value the response carries.
    #[must_use]
    pub fn as_header_value(&self) -> &http::HeaderValue {
        &self.0
    }
}

impl<C: Sync> crate::extract::FromRequestParts<C> for Option<AssignedId> {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut http::Parts,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<AssignedId>().cloned())
    }
}

/// Nothing: the header the identifier arrives in is the covering
/// `RequestId`'s to describe, and reading it uncovered reads `None`.
impl crate::extract::describe::Describe for Option<AssignedId> {
    fn describe(operation: &mut crate::router::operation::OperationCx<'_>) {
        let _ = operation;
    }
}

/// Assigns each request an identifier and echoes it back.
///
/// The header group `H` is both what every covered operation documents and
/// what the response carries. The identifier is also handed on as an
/// [`AssignedId`], which reads it without naming `H`.
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

    /// [`AssignedId::of`], spelled from the interceptor that assigns it.
    ///
    /// Reads the identifier any `RequestId` assigned, whatever its source and
    /// header group.
    #[must_use]
    pub fn of(request: &http::Request) -> Option<&AssignedId> {
        AssignedId::of(request)
    }

    /// [`AssignedId::of_response`], spelled from the interceptor that assigns
    /// it.
    ///
    /// Reads the identifier any `RequestId` wrote, whatever its source and
    /// header group.
    #[must_use]
    pub fn of_response(response: &http::Response) -> Option<&AssignedId> {
        AssignedId::of_response(response)
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

    /// Never answers with a response of its own: an identifier is added to
    /// whatever the chain returns, and a group that refuses to encode answers
    /// the 500 any refused group does, without running the chain.
    type Short = Infallible;

    const READS_BODY: bool = false;

    // Everything before the chain runs is done before the future is built, so
    // the future holds the rest of the chain rather than the request.
    fn intercept(
        &self,
        mut request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<H>, Infallible>> + Send {
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
        // A clone shares the value's one allocation.
        let assigned = AssignedId(id.clone());

        let headers = H::from_id(id);

        // Set on the request too, so handler, observer and client correlate on
        // one value under the same names. A group that refuses to encode is
        // refused on the response too, so the chain is not run for the 500
        // `with_headers` answers with. The other `Continued::new` outside
        // `Next::run` besides a cache hit: sound, because `with_headers` would
        // replace whatever the chain returned with this same 500.
        //
        // Encoded once: the response is written from the same fields, whose
        // values the request holds clones of. The extension is set only where
        // the fields are, so it never names an identifier no header carries.
        let run = match headers.encode() {
            Ok(fields) => {
                for (name, value) in &fields {
                    request.headers_mut().insert(name.clone(), value.clone());
                }
                request.extensions_mut().insert(assigned.clone());
                Ok((next.run(request), fields))
            }
            Err(_) => Err(headers),
        };

        async move {
            Ok(match run {
                Ok((run, fields)) => {
                    let mut continued = run.await.with_encoded::<H>(fields);
                    continued.insert_extension(assigned);
                    continued
                }
                Err(headers) => {
                    Continued::new(crate::response::headers::refused()).with_headers(headers)
                }
            })
        }
    }
}

#[cfg(test)]
mod tests;
