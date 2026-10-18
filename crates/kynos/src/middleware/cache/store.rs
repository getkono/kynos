//! What a cache stores, and where.

use std::{sync::Arc, time::Duration};

use kynos_openapi::Method;

use crate::http::{HeaderMap, HeaderValue, StatusCode};

/// What identifies a resource, before content negotiation.
///
/// Names the resource, not a variant: [`CacheStore::get`] returns every variant
/// filed under one of these, and Kynos selects among them (RFC 9111 section
/// 4.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrimaryKey {
    /// The application's namespace, set with
    /// [`Cache::namespace`](super::Cache::namespace).
    pub namespace: &'static str,
    /// The request method. `GET` or `HEAD` for anything stored.
    pub method: Method,
    /// The request's authority, lowercased: the target's (an absolute-form
    /// target, or a version-2 request's `:authority`), else `Host`. `None`
    /// where the request carried neither. The target's wins because RFC 9112
    /// section 3.2.2 has an origin server ignore `Host` beside one.
    ///
    /// The scheme is not part of the key: behind a terminating proxy a service
    /// cannot see the one the client used.
    pub authority: Option<String>,
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
    /// The request's field lines for each of those names, in the same order;
    /// none where the request did not send it.
    selecting: Vec<Box<[HeaderValue]>>,
    recorded_at: std::time::SystemTime,
    /// How long it may be reused without revalidation.
    freshness: Duration,
}

impl StoredResponse {
    /// Records a response varying on `vary`, selected by `request`'s values
    /// for those fields.
    pub(super) fn new(
        status: StatusCode,
        headers: HeaderMap,
        body: bytes::Bytes,
        vary: Vec<String>,
        request: &HeaderMap,
        freshness: Duration,
    ) -> Self {
        let selecting = vary
            .iter()
            .map(|name| request.get_all(name.as_str()).iter().cloned().collect())
            .collect();
        Self(Arc::new(Stored {
            status,
            headers,
            body,
            vary,
            selecting,
            recorded_at: std::time::SystemTime::now(),
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
        self.0.recorded_at.elapsed().unwrap_or_default()
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
    /// values that were present when it was stored, each compared "after
    /// combining multiple header field lines with the same field name": two
    /// lines match the one line that lists both, and a field sent empty does
    /// not match a field not sent.
    #[must_use]
    pub fn selected_by(&self, headers: &HeaderMap) -> bool {
        self.0
            .vary
            .iter()
            .zip(&self.0.selecting)
            .all(|(name, stored)| {
                let received = headers.get_all(name.as_str());
                received.iter().next().is_none() == stored.is_empty()
                    && combined(stored.iter()).eq(combined(received.iter()))
            })
    }

    /// Whether `other` is the same variant of a resource as this one, so that
    /// storing it replaces this one.
    ///
    /// The same variant varied on the same fields and was stored for the same
    /// request values of each, compared as [`selected_by`](Self::selected_by)
    /// compares them: two lines are the one line that lists both, and a field
    /// sent empty is not a field not sent. This is what
    /// [`CacheStore::put`] replaces by; [`vary`](Self::vary) alone is not,
    /// since every negotiation of a resource varies on the same names.
    #[must_use]
    pub fn same_variant(&self, other: &Self) -> bool {
        self.0.vary == other.0.vary
            && self
                .0
                .selecting
                .iter()
                .zip(&other.0.selecting)
                .all(|(mine, theirs)| {
                    mine.is_empty() == theirs.is_empty()
                        && combined(mine.iter()).eq(combined(theirs.iter()))
                })
    }
}

/// The octets of `lines` combined into one field value, as RFC 9110 section
/// 5.3 combines them: in order, separated by a comma and a space.
fn combined<'a>(lines: impl Iterator<Item = &'a HeaderValue>) -> impl Iterator<Item = u8> {
    lines.enumerate().flat_map(|(index, line)| {
        let separator: &[u8] = if index == 0 { b"" } else { b", " };
        separator.iter().chain(line.as_bytes()).copied()
    })
}

/// Where a [`Cache`](super::Cache) keeps responses.
///
/// The application supplies the store, as it does a
/// [`RateLimitStore`](crate::middleware::rate_limit::store::RateLimitStore).
///
/// # The contract
///
/// * `get` returns **every** stored variant of `key`. Kynos picks among them.
/// * `put` replaces the variant whose selecting values match, as
///   [`StoredResponse::same_variant`] judges them, and appends otherwise.
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
