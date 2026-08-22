//! Serving a response the operation already declares, from a store you supply.
//!
//! # How this module is laid out
//!
//! [`store`] is the seam and what goes through it, `freshness` decides what may
//! go through it, and the interceptor is here.
//!
//! # Where a cache sits
//!
//! Outermost but one. Mount [`Conditional`](super::conditional::Conditional)
//! *outside* this, so a hit is turned into a 304 having produced only the
//! cached body; mount this outside `Cors` and `Compression`, so what is stored
//! is a response whose negotiated headers have already landed.
//!
//! The order is documented rather than enforced. Enforcing it needs a marker
//! threaded through `CompatibleWith` for one interceptor, which generalizes the
//! `Cors` downcast into the capability `docs/middleware.md` explicitly bounds.
//! What *is* enforced is the case where getting it wrong is catastrophic: a
//! response carrying CORS headers whose `Vary` does not name `origin` is
//! refused outright, because storing one hands one origin's
//! `Access-Control-Allow-Origin` to another.

pub mod store;

mod freshness;

use std::{convert::Infallible, marker::PhantomData, time::Duration};

use kynos_openapi::model::schema::types::SchemaType;

pub use store::{CacheStore, PrimaryKey, StoredResponse};

use crate::{
    extract::params::header::HeaderParams,
    http::{self, HeaderMap, HeaderValue, header},
    middleware::{Continued, Interceptor, Next},
    schema::registry::Registry,
};

/// How large a body may be and still be stored.
const DEFAULT_MAX_BODY_BYTES: u64 = 1024 * 1024;

mod sealed {
    pub trait Sealed {}
}

/// Whether a cache derives an entity tag for a response that carries none.
///
/// Sealed, and there are exactly two. Reached through
/// [`Cache::deriving_etags`], which changes the type because it changes what
/// every covered operation declares.
pub trait CacheTagging: sealed::Sealed + Send + Sync + 'static {
    /// The group a served response carries.
    type Headers: HeaderParams;

    /// Whether a tag is derived.
    const DERIVES: bool;

    /// Builds the group.
    fn headers(age: Duration, etag: Option<String>) -> Self::Headers;
}

/// A cache that leaves validators to whatever produced the response.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plain;

/// A cache that derives a strong entity tag for a stored response carrying
/// none.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tagged;

impl sealed::Sealed for Plain {}
impl sealed::Sealed for Tagged {}

/// What a [`Cache`] adds to a response.
///
/// `Age` is not described. It is a cache-to-cache field, and putting it in a
/// generated client would be telling a consumer about something none of them
/// act on — the same judgement `Vary` and the CORS set already get.
#[derive(Clone, Debug, Default)]
pub struct CacheHeaders<const TAGGED: bool = false> {
    age: Duration,
    etag: Option<String>,
}

impl<const TAGGED: bool> HeaderParams for CacheHeaders<TAGGED> {
    const NAMES: &'static [&'static str] = if TAGGED { &["age", "etag"] } else { &["age"] };
    const DESCRIBED: bool = TAGGED;

    fn encode(&self) -> Vec<(http::HeaderName, HeaderValue)> {
        let mut fields = Vec::with_capacity(2);

        if let Ok(value) = HeaderValue::from_str(&self.age.as_secs().to_string()) {
            fields.push((header::AGE, value));
        }
        if TAGGED {
            if let Some(value) = self
                .etag
                .as_deref()
                .and_then(|etag| HeaderValue::from_str(etag).ok())
            {
                fields.push((header::ETAG, value));
            }
        }

        fields
    }

    /// Only `ETag`, and only where one is derived.
    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        let mut headers = kynos_openapi::Map::new();
        if TAGGED {
            headers.insert(
                "ETag".to_owned(),
                kynos_openapi::RefOr::Item(
                    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
                        .with_description("The entity tag of this representation"),
                ),
            );
        }
        headers
    }
}

impl CacheTagging for Plain {
    type Headers = CacheHeaders<false>;
    const DERIVES: bool = false;

    fn headers(age: Duration, etag: Option<String>) -> Self::Headers {
        let _ = etag;
        CacheHeaders { age, etag: None }
    }
}

impl CacheTagging for Tagged {
    type Headers = CacheHeaders<true>;
    const DERIVES: bool = true;

    fn headers(age: Duration, etag: Option<String>) -> Self::Headers {
        CacheHeaders { age, etag }
    }
}

/// Serves responses the operation already declares, from a store you supply.
///
/// `Short` is [`Infallible`]: a hit replays a status the operation already
/// produced, so a cache contributes no response of its own. What it adds is
/// `Age`, and — under [`Tagged`] — an `ETag` that makes
/// [`Conditional`](super::conditional::Conditional) useful for a handler that
/// declares no validator.
///
/// ```no_run
/// use kynos::middleware::cache::{Cache, CacheStore, PrimaryKey, StoredResponse};
/// # struct MyStore;
/// # impl CacheStore<()> for MyStore {
/// #     async fn get(&self, _: &PrimaryKey, _: &()) -> Vec<StoredResponse> { Vec::new() }
/// #     async fn put(&self, _: PrimaryKey, _: StoredResponse, _: &()) {}
/// #     async fn invalidate(&self, _: &PrimaryKey, _: &()) {}
/// # }
///
/// let cache = Cache::new(MyStore).namespace("v1").deriving_etags();
/// # let _ = cache;
/// ```
#[derive(Clone, Debug)]
pub struct Cache<S, D = Plain> {
    store: S,
    namespace: &'static str,
    max_body_bytes: u64,
    default_freshness: Option<Duration>,
    _tagging: PhantomData<fn() -> D>,
}

impl<S> Cache<S, Plain> {
    /// Caches through `store`.
    #[must_use]
    pub fn new(store: S) -> Self {
        Self {
            store,
            namespace: "",
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            default_freshness: None,
            _tagging: PhantomData,
        }
    }

    /// Also derives a strong `ETag` for a stored response carrying none.
    ///
    /// Changes the type, because it changes what every covered operation
    /// declares — and because mounting this beside anything else setting `ETag`
    /// has to be a compile error rather than a response with two.
    #[must_use]
    pub fn deriving_etags(self) -> Cache<S, Tagged> {
        Cache {
            store: self.store,
            namespace: self.namespace,
            max_body_bytes: self.max_body_bytes,
            default_freshness: self.default_freshness,
            _tagging: PhantomData,
        }
    }
}

impl<S, D> Cache<S, D> {
    /// Prefixes every key.
    ///
    /// Bump it on a deploy that changes what an operation returns: a store that
    /// outlives a process can otherwise serve a response the new binary no
    /// longer declares.
    #[must_use]
    pub fn namespace(mut self, namespace: &'static str) -> Self {
        self.namespace = namespace;
        self
    }

    /// Refuses to store a body larger than `bytes`. One mebibyte by default.
    #[must_use]
    pub fn max_body_bytes(mut self, bytes: u64) -> Self {
        self.max_body_bytes = bytes;
        self
    }

    /// A freshness lifetime for a response that stated none.
    ///
    /// Off by default. RFC 9111 section 4.2.2 permits a heuristic and every
    /// heuristic is a guess that turns a correct origin into an incorrect
    /// cache, so this is a number a deployment supplies rather than one Kynos
    /// invents.
    #[must_use]
    pub fn default_freshness(mut self, lifetime: Duration) -> Self {
        self.default_freshness = Some(lifetime);
        self
    }
}

impl<C, S, D> Interceptor<C> for Cache<S, D>
where
    C: Sync + 'static,
    S: CacheStore<C>,
    D: CacheTagging,
{
    type Reads = ();
    type Adds = D::Headers;
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<D::Headers>, Infallible> {
        let () = reads;

        let route = next.route();
        let key = PrimaryKey {
            namespace: self.namespace,
            method: kynos_openapi::Method::from_wire_str(request.method().as_str())
                .unwrap_or(kynos_openapi::Method::Get),
            route: route.path().to_owned(),
            target: request
                .uri()
                .path_and_query()
                .map_or_else(|| request.uri().path().to_owned(), ToString::to_string),
        };

        let request_headers = request.headers().clone();
        let method = request.method().clone();

        // A hit replays a status the operation already declares, which is why
        // `Short` is `Infallible`: nothing here invents a response.
        if let Some(stored) = self
            .store
            .get(&key, context)
            .await
            .into_iter()
            .filter(|stored| stored.selected_by(&request_headers))
            .find(StoredResponse::is_fresh)
        {
            let age = stored.age();
            let etag = stored
                .headers()
                .get(header::ETAG)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
                .or_else(|| D::DERIVES.then(|| derived_etag(stored.body())));

            let mut response =
                http::Response::new(crate::http::body::Body::from_bytes(stored.body().clone()));
            *response.status_mut() = stored.status();
            *response.headers_mut() = stored.headers().clone();

            // `Continued::new` is `pub(crate)`, and this is the one place
            // outside `Next::run` that calls it. The invariant it protects --
            // an interceptor either forwards what the chain produced or answers
            // with its declared `Short` -- is not weakened: a hit replays a
            // response *this operation produced*, stored only after it passed
            // the storability rules. A third-party interceptor still cannot
            // mint one, because the constructor is not public.
            return Ok(Continued::new(response).with_headers(D::headers(age, etag)));
        }

        let mut continued = next.run(request).await;

        let Ok(freshness) = freshness::storable(
            &method,
            continued.status(),
            &request_headers,
            continued.headers(),
            self.default_freshness,
        ) else {
            return Ok(continued.with_headers(D::headers(Duration::ZERO, None)));
        };

        // A body that cannot state its length is a stream, and buffering one to
        // cache it defeats the reason it is a stream. The same sentence
        // `Compression` uses.
        let body = continued.take_body();
        let Some(bytes) = bounded(body, self.max_body_bytes).await else {
            continued.set_body(crate::http::body::Body::empty());
            return Ok(continued.with_headers(D::headers(Duration::ZERO, None)));
        };

        let mut headers = continued.headers().clone();
        freshness::strip(&mut headers);

        let etag = headers
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
            .or_else(|| D::DERIVES.then(|| derived_etag(&bytes)));

        if refuses_cross_origin(&headers) {
            // The mis-ordering case, caught without needing to know the order:
            // a response carrying CORS headers whose `Vary` does not name
            // `origin` would hand one origin's answer to another.
            continued.set_body(crate::http::body::Body::from_bytes(bytes));
            return Ok(continued.with_headers(D::headers(Duration::ZERO, etag)));
        }

        let vary = freshness::vary(&headers);
        let selecting = vary
            .iter()
            .map(|name| request_headers.get(name.as_str()).cloned())
            .collect();

        self.store
            .put(
                key,
                StoredResponse::new(
                    continued.status(),
                    headers,
                    bytes.clone(),
                    vary,
                    selecting,
                    freshness,
                ),
                context,
            )
            .await;

        continued.set_body(crate::http::body::Body::from_bytes(bytes));
        Ok(continued.with_headers(D::headers(Duration::ZERO, etag)))
    }
}

/// A strong entity tag over a body.
///
/// FNV-1a with the length folded in, for the reason
/// [`assets`](crate::router::assets) gives: a validator is not a security
/// primitive, and a cryptographic hash would mean a dependency carrying
/// `unsafe`.
fn derived_etag(body: &bytes::Bytes) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let hashed = body.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    });

    format!(
        "\"{:016x}\"",
        (hashed ^ (body.len() as u64)).wrapping_mul(PRIME)
    )
}

/// Whether storing this response would risk serving one origin's answer to
/// another.
fn refuses_cross_origin(headers: &HeaderMap) -> bool {
    let cross_origin = headers
        .keys()
        .any(|name| name.as_str().starts_with("access-control-"));

    cross_origin && !freshness::vary(headers).iter().any(|name| name == "origin")
}

/// Reads a body whole, or `None` where it is longer than `limit` or its length
/// is unknown.
async fn bounded(body: crate::http::body::Body, limit: u64) -> Option<bytes::Bytes> {
    use http_body::Body as _;

    if body.size_hint().exact()? > limit {
        return None;
    }

    http_body_util::BodyExt::collect(body)
        .await
        .ok()
        .map(http_body_util::Collected::to_bytes)
}

#[cfg(test)]
mod tests;
