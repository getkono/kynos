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

use std::{borrow::Cow, convert::Infallible, marker::PhantomData, time::Duration};

use kynos_openapi::model::schema::types::SchemaType;

use crate::middleware::cache::store::{CacheStore, PrimaryKey, StoredResponse};
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{self, HeaderMap, HeaderValue, header},
    middleware::{Continued, Interceptor, Next},
    schema::registry::Registry,
};

/// How large a body may be and still be stored.
const DEFAULT_MAX_BODY_BYTES: u64 = 1024 * 1024;

mod sealed {
    pub trait Sealed {}

    /// Writes a cache's group onto a response's fields directly, skipping the
    /// list [`EncodeHeaders::encode`](super::EncodeHeaders::encode) builds.
    pub trait Written {
        fn write(self, fields: &mut super::HeaderMap);
    }
}

/// Whether a cache derives an entity tag for a response that carries none.
///
/// Sealed, and there are exactly two. Reached through
/// [`Cache::deriving_etags`], which changes the type because it changes what
/// every covered operation declares.
pub trait CacheTagging: sealed::Sealed + Send + Sync + 'static {
    /// The group a served response carries.
    type Headers: EncodeHeaders + sealed::Written;

    /// Whether a tag is derived.
    const DERIVES: bool;

    /// Builds the group.
    fn headers(age: Duration, etag: Option<HeaderValue>) -> Self::Headers;
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
    etag: Option<HeaderValue>,
}

impl<const TAGGED: bool> CacheHeaders<TAGGED> {
    /// Every field the group sends, in the order it sends them.
    fn into_fields(self) -> impl Iterator<Item = (http::HeaderName, HeaderValue)> {
        // Both values come from the stored response, the derived tag or a
        // count of seconds, so neither is ever refused. A zero age, which a
        // miss always carries, is a static value rather than a new one.
        let age = match self.age.as_secs() {
            0 => HeaderValue::from_static("0"),
            seconds => HeaderValue::from(seconds),
        };
        let etag = self.etag.filter(|_| TAGGED);

        std::iter::once((header::AGE, age)).chain(etag.map(|etag| (header::ETAG, etag)))
    }
}

impl<const TAGGED: bool> sealed::Written for CacheHeaders<TAGGED> {
    fn write(self, fields: &mut HeaderMap) {
        for (name, value) in self.into_fields() {
            fields.insert(name, value);
        }
    }
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
    // A cache writes its group with `sealed::Written`; this is the same
    // fields, for any other writer.
    fn encode(&self) -> Result<Vec<(http::HeaderName, HeaderValue)>, UnencodableHeader> {
        Ok(self.clone().into_fields().collect())
    }
}

impl CacheTagging for Plain {
    type Headers = CacheHeaders<false>;
    const DERIVES: bool = false;

    fn headers(age: Duration, etag: Option<HeaderValue>) -> Self::Headers {
        let _ = etag;
        CacheHeaders { age, etag: None }
    }
}

impl CacheTagging for Tagged {
    type Headers = CacheHeaders<true>;
    const DERIVES: bool = true;

    fn headers(age: Duration, etag: Option<HeaderValue>) -> Self::Headers {
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
/// #     async fn get(&self, _: &PrimaryKey<'_>, _: &()) -> Vec<StoredResponse> { Vec::new() }
/// #     async fn put(&self, _: PrimaryKey<'static>, _: StoredResponse, _: &()) {}
/// #     async fn invalidate(&self, _: &PrimaryKey<'static>, _: &()) {}
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
    /// A response stating one through `s-maxage`, `max-age` or `Expires` is
    /// never given this, even where what it stated is unreadable, repeated or
    /// already past: RFC 9111 section 4.2.1 reads that response as stale.
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

    // A block rather than an `async fn`, whose body rebinds each argument and
    // so keeps the request twice across the store's lookup.
    fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<D::Headers>, Infallible>> + Send {
        let () = reads;

        async move {
            // A key names an operation, and a fallback has none to store under.
            // A method OpenAPI has no field for is never routed, so a routed
            // request always names one.
            let (Some(route), Some(keyed)) = (
                next.route().map(|route| route.path()),
                kynos_openapi::Method::from_wire_str(request.method().as_str()),
            ) else {
                return Ok(next.run(request).await.unrouted());
            };

            // A request no router dispatched carries no record and is read as
            // guarded: refusing a store is the safe error.
            let secured = request
                .extensions()
                .get::<crate::router::dispatch::Routed>()
                .is_none_or(|routed| routed.secured);
            let requested = freshness::Requested::of(request.headers());
            let method = request.method().clone();
            let storing = freshness::storable_method(&method) && !requested.forbids_storing();

            // What outlives the request: the key, owned, where a store or an
            // invalidation may name it, and the request's fields where a store
            // may select by them. Which fields that is, the response's `Vary`
            // says, and it has not been written yet. Scoped, so the future does
            // not also keep the borrowed key beside the owned one.
            let (key, selecting) = {
                // Borrows the request: a lookup allocates no key.
                let key = primary_key(self.namespace, keyed, &request, route);

                // A hit is served before the operation's guard runs, so for a
                // guarded operation only a response that said it may be shared
                // is a hit.
                if !requested.forbids_reuse() {
                    let stored = self
                        .store
                        .get(&key, context)
                        .await
                        .into_iter()
                        .filter(|stored| stored.selected_by(request.headers()))
                        .filter(|stored| {
                            !secured || freshness::servable_when_secured(stored.headers())
                        })
                        .find(StoredResponse::is_fresh);
                    if let Some(stored) = stored {
                        return Ok(served::<D>(&stored));
                    }
                }

                (
                    (storing || !method.is_safe()).then(|| key.into_owned()),
                    storing.then(|| request.headers().clone()),
                )
            };

            let mut continued = next.run(request).await;

            // RFC 9111 section 4.4 invalidates the target URI on a non-error status
            // to an unsafe (or unknown) method; the URI's stored entries are its
            // `GET` and `HEAD` keys, not the key this request's method would build.
            if let Some(key) = key
                .as_ref()
                .filter(|_| !method.is_safe() && is_non_error(continued.status()))
            {
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

            // Read in place rather than bound anew, which would keep a second
            // copy of each in the future.
            let Some(selecting) = selecting.as_ref() else {
                return Ok(added::<D>(continued, Duration::ZERO, None));
            };
            let Ok(freshness) = freshness::storable(
                &method,
                continued.status(),
                requested,
                continued.headers(),
                secured,
                self.default_freshness,
            ) else {
                return Ok(added::<D>(continued, Duration::ZERO, None));
            };

            // A body not buffered is forwarded as it arrived, and one whose read
            // failed is forwarded failing.
            let bytes = match bounded(continued.take_body(), self.max_body_bytes).await {
                Ok(bytes) => bytes,
                Err(unbuffered) => {
                    continued.set_body(unbuffered);
                    return Ok(added::<D>(continued, Duration::ZERO, None));
                }
            };

            let mut headers = continued.headers().clone();
            freshness::strip(&mut headers);

            let etag = tag::<D>(&headers, &bytes);

            if refuses_cross_origin(&headers) {
                continued.set_body(crate::http::body::Body::from_bytes(bytes));
                return Ok(added::<D>(continued, Duration::ZERO, etag));
            }

            let vary = freshness::vary(&headers);
            let stored = StoredResponse::new(
                continued.status(),
                headers,
                bytes.clone(),
                vary,
                selecting,
                freshness,
            );

            // Always present beside `selecting`.
            if let Some(key) = key {
                self.store.put(key, stored, context).await;
            }

            continued.set_body(crate::http::body::Body::from_bytes(bytes));
            Ok(added::<D>(continued, Duration::ZERO, etag))
        }
    }
}

/// `continued` with the group a cache under `D` adds, written onto it
/// directly.
fn added<D: CacheTagging>(
    continued: Continued,
    age: Duration,
    etag: Option<HeaderValue>,
) -> Continued<D::Headers> {
    continued.with_written::<D::Headers>(|fields| {
        sealed::Written::write(D::headers(age, etag), fields);
    })
}

/// A hit, replayed with the group a cache adds.
fn served<D: CacheTagging>(stored: &StoredResponse) -> Continued<D::Headers> {
    let etag = tag::<D>(stored.headers(), stored.body());

    let mut response =
        http::Response::new(crate::http::body::Body::from_bytes(stored.body().clone()));
    *response.status_mut() = stored.status();
    *response.headers_mut() = stored.headers().clone();

    // A `Continued::new` outside `Next::run` (the other is `RequestId`'s
    // refused group): sound, because a hit replays a response this operation
    // itself produced.
    added::<D>(Continued::new(response), stored.age(), etag)
}

/// The entity tag a cache under `D` sends beside `headers` and `body`: the one
/// they carry, else a derived one. `None` where `D` sends none.
fn tag<D: CacheTagging>(headers: &HeaderMap, body: &bytes::Bytes) -> Option<HeaderValue> {
    if !D::DERIVES {
        return None;
    }

    // A tag that is not visible ASCII is replaced, as one that is absent is.
    let carried = headers
        .get(header::ETAG)
        .filter(|value| value.to_str().is_ok())
        .cloned();
    carried.or_else(|| HeaderValue::try_from(derived_etag(body)).ok())
}

/// What `request`, sent with `method`, is filed under, for the operation whose
/// `paths` key is `route`.
fn primary_key<'a>(
    namespace: &'static str,
    method: kynos_openapi::Method,
    request: &'a http::Request,
    route: &'a str,
) -> PrimaryKey<'a> {
    PrimaryKey {
        namespace,
        method,
        authority: crate::http::own_authority(
            request.headers(),
            request
                .uri()
                .authority()
                .map(::http::uri::Authority::as_str),
        ),
        route: Cow::Borrowed(route),
        target: Cow::Borrowed(
            request
                .uri()
                .path_and_query()
                .map_or_else(|| request.uri().path(), ::http::uri::PathAndQuery::as_str),
        ),
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
    mut body: crate::http::body::Body,
    limit: u64,
) -> Result<bytes::Bytes, crate::http::body::Body> {
    if body.exact_length().is_none_or(|length| length > limit) {
        return Err(body);
    }

    http_body_util::BodyExt::collect(body)
        .await
        .map(http_body_util::Collected::to_bytes)
        .map_err(crate::http::body::Body::failed)
}

#[cfg(test)]
mod tests;
