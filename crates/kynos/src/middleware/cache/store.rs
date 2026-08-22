//! What a cache stores, and where.

use std::{sync::Arc, time::Duration};

use kynos_openapi::Method;

use crate::http::{HeaderMap, HeaderValue, StatusCode};

/// What identifies a resource, before content negotiation.
///
/// Not a variant: a resource may have several representations and this names
/// the resource. [`CacheStore::get`] returns every variant filed under one of
/// these, and Kynos picks among them — because only a stored response knows
/// what it varied on, and a store that selected would have to reimplement
/// RFC 9111 section 4.1 once per store.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrimaryKey {
    /// The application's namespace.
    ///
    /// Bump it on a deploy that changes what an operation returns. A store that
    /// outlives a process can hold a response the new binary no longer
    /// declares, and that would breach *emitted ⊇ observable* — one line here
    /// is the remedy, and it is a deploy step rather than something the type
    /// system can carry.
    pub namespace: &'static str,
    /// The request method. `GET` or `HEAD` for anything stored.
    pub method: Method,
    /// The `paths` key, not the request path.
    pub route: String,
    /// The request target, path and query, exactly as it arrived.
    pub target: String,
}

/// A response held by a store.
///
/// Cloning is cheap: the body is [`Bytes`](bytes::Bytes) and the rest is behind
/// an `Arc`.
#[derive(Clone, Debug)]
pub struct StoredResponse(Arc<Stored>);

#[derive(Debug)]
struct Stored {
    status: StatusCode,
    headers: HeaderMap,
    body: bytes::Bytes,
    /// The field names the response varied on, lowercased and sorted.
    vary: Vec<String>,
    /// The request's values for those names, in the same order.
    selecting: Vec<Option<HeaderValue>>,
    stored_at: std::time::SystemTime,
    /// How long it may be reused without revalidation.
    freshness: Duration,
}

impl StoredResponse {
    /// Records a response and what selected it.
    pub(super) fn new(
        status: StatusCode,
        headers: HeaderMap,
        body: bytes::Bytes,
        vary: Vec<String>,
        selecting: Vec<Option<HeaderValue>>,
        freshness: Duration,
    ) -> Self {
        Self(Arc::new(Stored {
            status,
            headers,
            body,
            vary,
            selecting,
            stored_at: std::time::SystemTime::now(),
            freshness,
        }))
    }

    /// The stored status.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.0.status
    }

    /// The stored headers, hop-by-hop fields already removed.
    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        &self.0.headers
    }

    /// The stored body.
    #[must_use]
    pub fn body(&self) -> &bytes::Bytes {
        &self.0.body
    }

    /// The field names this response varied on.
    #[must_use]
    pub fn vary(&self) -> &[String] {
        &self.0.vary
    }

    /// How long ago it was stored.
    #[must_use]
    pub fn age(&self) -> Duration {
        self.0.stored_at.elapsed().unwrap_or_default()
    }

    /// Whether it may still be reused without revalidation.
    #[must_use]
    pub fn is_fresh(&self) -> bool {
        self.age() < self.0.freshness
    }

    /// Whether this variant is the one `headers` selects.
    ///
    /// RFC 9111 section 4.1: a stored response matches when the request's
    /// values for every field the *stored* response varied on agree with the
    /// values that were present when it was stored.
    #[must_use]
    pub fn selected_by(&self, headers: &HeaderMap) -> bool {
        self.0
            .vary
            .iter()
            .zip(&self.0.selecting)
            .all(|(name, stored)| headers.get(name.as_str()) == stored.as_ref())
    }
}

/// Where a [`Cache`](super::Cache) keeps responses.
///
/// The store is the application's, for the reason
/// [`RateLimitStore`](crate::middleware::rate_limit::RateLimitStore) is:
/// prescribing one would mean prescribing a dependency.
///
/// # The contract
///
/// * `get` returns **every** stored variant of `key`. Kynos picks among them.
/// * `put` replaces the variant whose selecting values match, and appends
///   otherwise.
/// * `invalidate` drops every variant, because RFC 9111 section 4.4
///   invalidates a URI rather than one negotiation of it.
pub trait CacheStore<C>: Send + Sync + 'static {
    /// Every stored variant of `key`.
    fn get(
        &self,
        key: &PrimaryKey,
        context: &C,
    ) -> impl Future<Output = Vec<StoredResponse>> + Send;

    /// Stores `response` under `key`.
    fn put(
        &self,
        key: PrimaryKey,
        response: StoredResponse,
        context: &C,
    ) -> impl Future<Output = ()> + Send;

    /// Drops every stored variant of `key`.
    fn invalidate(&self, key: &PrimaryKey, context: &C) -> impl Future<Output = ()> + Send;
}
