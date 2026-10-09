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
//! is a response whose negotiated headers have already landed. Outside is the
//! *earlier* `intercept` call, per
//! [the module's ordering rule](super#the-order-a-chain-runs-in); a hit never
//! reaches an interceptor mounted inside the cache.
//!
//! The order is documented rather than enforced, except that a response
//! carrying CORS headers whose `Vary` does not name `origin` is never stored,
//! since it would hand one origin's `Access-Control-Allow-Origin` to another.

pub mod store;

mod freshness;

use std::{convert::Infallible, marker::PhantomData, time::Duration};

use kynos_openapi::model::schema::types::SchemaType;

use crate::middleware::cache::store::{CacheStore, PrimaryKey, StoredResponse};
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
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
    type Headers: EncodeHeaders;

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
/// `Age` is not described: it is a cache-to-cache field no generated client
/// acts on.
#[derive(Clone, Debug, Default)]
pub struct CacheHeaders<const TAGGED: bool = false> {
    age: Duration,
    etag: Option<String>,
}

impl<const TAGGED: bool> HeaderParams for CacheHeaders<TAGGED> {
    const NAMES: &'static [&'static str] = if TAGGED { &["age", "etag"] } else { &["age"] };
    const DESCRIBED: bool = TAGGED;
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

impl<const TAGGED: bool> EncodeHeaders for CacheHeaders<TAGGED> {
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
/// A stored response is filed under the request's authority as well as its
/// target, and a request saying `Cache-Control: no-cache` is answered by the
/// handler, since a cache that does not revalidate cannot honour it otherwise.
///
/// A response to an operation declaring a security requirement — through
/// [`Auth`](crate::security::auth::Auth), [`MaybeAuth`](crate::security::auth::MaybeAuth)
/// or any other guard — is stored and served only where it says `public` or
/// `s-maxage`, as one to a request carrying `Authorization` is: a hit is served
/// before the guard runs, so whatever credential carried the request, nothing
/// else keeps one caller's answer from another.
///
/// ```no_run
/// use kynos::middleware::cache::{
///     Cache,
///     store::{CacheStore, PrimaryKey, StoredResponse},
/// };
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
    /// Changes the type, since every covered operation then declares `ETag`;
    /// mounting it beside anything else setting `ETag` is a compile error.
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

    /// Refuses to store a body larger than `bytes`, or one that cannot state its
    /// length; either is forwarded untouched. One mebibyte by default.
    #[must_use]
    pub fn max_body_bytes(mut self, bytes: u64) -> Self {
        self.max_body_bytes = bytes;
        self
    }

    /// A freshness lifetime for a response that stated none.
    ///
    /// Off by default: Kynos applies no RFC 9111 section 4.2.2 heuristic, so
    /// this is a guess only a deployment can make.
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

        let key = primary_key(self.namespace, &request, next.route().path());
        // A request no router dispatched carries no record and is read as
        // guarded: refusing a store is the safe error.
        let secured = request
            .extensions()
            .get::<crate::router::dispatch::Routed>()
            .is_none_or(|routed| routed.secured);

        let request_headers = request.headers().clone();
        let method = request.method().clone();

        // A hit is served before the operation's guard runs, so for a guarded
        // operation only a response that said it may be shared is a hit.
        let stored = if freshness::forbids_reuse(&request_headers) {
            None
        } else {
            self.store
                .get(&key, context)
                .await
                .into_iter()
                .filter(|stored| stored.selected_by(&request_headers))
                .filter(|stored| !secured || freshness::servable_when_secured(stored.headers()))
                .find(StoredResponse::is_fresh)
        };
        if let Some(stored) = stored {
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

            // The one `Continued::new` outside `Next::run`: sound, because a hit
            // replays a response this operation itself produced.
            return Ok(Continued::new(response).with_headers(D::headers(age, etag)));
        }

        let mut continued = next.run(request).await;

        // RFC 9111 section 4.4 invalidates the target URI on a non-error status
        // to an unsafe (or unknown) method; the URI's stored entries are its
        // `GET` and `HEAD` keys, not the key this request's method would build.
        if !method.is_safe() && is_non_error(continued.status()) {
            for stored in [kynos_openapi::Method::Get, kynos_openapi::Method::Head] {
                self.store
                    .invalidate(
                        &PrimaryKey {
                            method: stored,
                            ..key.clone()
                        },
                        context,
                    )
                    .await;
            }
        }

        let Ok(freshness) = freshness::storable(
            &method,
            continued.status(),
            &request_headers,
            continued.headers(),
            secured,
            self.default_freshness,
        ) else {
            return Ok(continued.with_headers(D::headers(Duration::ZERO, None)));
        };

        // A body not buffered is forwarded as it arrived, and one whose read
        // failed is forwarded failing.
        let bytes = match bounded(continued.take_body(), self.max_body_bytes).await {
            Ok(bytes) => bytes,
            Err(unbuffered) => {
                continued.set_body(unbuffered);
                return Ok(continued.with_headers(D::headers(Duration::ZERO, None)));
            }
        };

        let mut headers = continued.headers().clone();
        freshness::strip(&mut headers);

        let etag = headers
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
            .or_else(|| D::DERIVES.then(|| derived_etag(&bytes)));

        if refuses_cross_origin(&headers) {
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

/// What `request` is filed under, for the operation whose `paths` key is
/// `route`.
fn primary_key(namespace: &'static str, request: &http::Request, route: &str) -> PrimaryKey {
    PrimaryKey {
        namespace,
        method: kynos_openapi::Method::from_wire_str(request.method().as_str())
            .unwrap_or(kynos_openapi::Method::Get),
        authority: crate::middleware::csrf::own_authority(
            request.headers(),
            request
                .uri()
                .authority()
                .map(::http::uri::Authority::as_str),
        ),
        route: route.to_owned(),
        target: request
            .uri()
            .path_and_query()
            .map_or_else(|| request.uri().path().to_owned(), ToString::to_string),
    }
}

/// Whether `status` is a non-error (2xx or 3xx) status, per RFC 9111 section
/// 4.4.
fn is_non_error(status: http::StatusCode) -> bool {
    status.is_success() || status.is_redirection()
}

/// A strong entity tag over a body.
///
/// FNV-1a with the length folded in: a validator is not a security primitive.
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

/// Reads a body whole, or hands it back unread where its length is unknown or
/// past `limit`.
///
/// Decided on the size hint, so a declined body is unconsumed. A read failing
/// part-way comes back as a body failing the same way: RFC 9111 section 3.3
/// forbids sending an incomplete response unmarked.
async fn bounded(
    body: crate::http::body::Body,
    limit: u64,
) -> Result<bytes::Bytes, crate::http::body::Body> {
    use http_body::Body as _;

    if body.size_hint().exact().is_none_or(|length| length > limit) {
        return Err(body);
    }

    http_body_util::BodyExt::collect(body)
        .await
        .map(http_body_util::Collected::to_bytes)
        .map_err(crate::http::body::Body::failed)
}

#[cfg(test)]
mod tests;
