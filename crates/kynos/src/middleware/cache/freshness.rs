//! What a shared cache may store, and for how long.

use std::time::Duration;

use crate::http::{HeaderMap, StatusCode, header};

/// Statuses a response may be stored under.
///
/// RFC 9110 section 15.1's heuristically-cacheable set, minus 206: this cache
/// stores whole responses and cannot recombine parts (section 14.4).
pub(super) const CACHEABLE: &[u16] = &[200, 203, 204, 300, 301, 308, 404, 405, 410, 414, 501];

/// Fields a stored response must not keep.
///
/// RFC 9110 section 7.6.1's connection-specific fields, plus `Age`, which is
/// recomputed on the way out.
pub(super) const HOP_BY_HOP: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "age",
];

/// Why a response was not stored; a closed set the table test counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unstorable {
    /// The method is neither `GET` nor `HEAD`.
    Method,
    /// The status is not in [`CACHEABLE`].
    Status,
    /// The request said `no-store`.
    RequestNoStore,
    /// The response said `no-store`.
    ResponseNoStore,
    /// The response said `private`, and this is a shared cache.
    Private,
    /// The response said `no-cache`, which forbids reuse without revalidation
    /// — and Kynos does not revalidate.
    NoCache,
    /// `Vary: *`, which says the response depends on more than field names can
    /// express.
    VaryWildcard,
    /// The response sets a cookie.
    SetCookie,
    /// The request carried `Authorization`, or the operation declares a
    /// security requirement, and the response did not say it was shareable.
    Authorized,
    /// The response said nothing about how long it may be reused, and no
    /// default was configured.
    NoFreshness,
    // No `Body` variant: `bounded` caps a body before `storable` is asked.
}

/// Whether a response may be stored, and for how long.
///
/// `secured` is whether the operation declares a security requirement, as
/// routing recorded it for the request.
pub(super) fn storable(
    method: &crate::http::Method,
    status: StatusCode,
    request: &HeaderMap,
    response: &HeaderMap,
    secured: bool,
    default_freshness: Option<Duration>,
) -> Result<Duration, Unstorable> {
    // RFC 9111 section 3 permits caching `POST` only with `Content-Location`;
    // not supported.
    if !matches!(
        method,
        &crate::http::Method::GET | &crate::http::Method::HEAD
    ) {
        return Err(Unstorable::Method);
    }

    if !CACHEABLE.contains(&status.as_u16()) {
        return Err(Unstorable::Status);
    }

    let request_control = directives(request);
    if request_control.iter().any(|value| value == "no-store") {
        return Err(Unstorable::RequestNoStore);
    }

    let response_control = directives(response);
    for (directive, refusal) in [
        ("no-store", Unstorable::ResponseNoStore),
        ("private", Unstorable::Private),
        ("no-cache", Unstorable::NoCache),
    ] {
        // A field-narrowed `private` or `no-cache` counts as the whole one:
        // this cache cannot store part of a response.
        if response_control
            .iter()
            .any(|value| value == directive || value.starts_with(&format!("{directive}=")))
        {
            return Err(refusal);
        }
    }

    // Every line, as `vary` reads them: a split list carries `*` on any of them.
    if vary(response).iter().any(|name| name == "*") {
        return Err(Unstorable::VaryWildcard);
    }

    // No opt-out: `Vary` cannot keep a minted session from a second client.
    if response.contains_key(header::SET_COOKIE) {
        return Err(Unstorable::SetCookie);
    }

    // RFC 9111 section 3.5: a response to an authenticated request is shared
    // only where it says so. Other credentials are seen only through the
    // declared requirement, which counts even where it admits anonymous access.
    if (secured || request.contains_key(header::AUTHORIZATION)) && !shared(&response_control) {
        return Err(Unstorable::Authorized);
    }

    freshness(&response_control, default_freshness).ok_or(Unstorable::NoFreshness)
}

/// Whether a stored response may be served for an operation declaring a
/// security requirement.
///
/// Rechecked on read because the store outlives the rule: a response stored
/// before a guard was added must not be served past it.
pub(super) fn servable_when_secured(stored: &HeaderMap) -> bool {
    shared(&directives(stored))
}

/// Whether the directives say a shared cache may reuse the response for any
/// requester, as RFC 9111 section 3.5 lists them for an authenticated request.
/// `must-revalidate` is not read, since this cache does not revalidate.
fn shared(control: &[String]) -> bool {
    control
        .iter()
        .any(|value| value == "public" || value.starts_with("s-maxage="))
}

/// Whether the request forbids answering it from the store.
///
/// RFC 9111 section 5.2.1.4 `no-cache`, since this cache does not validate.
/// `Pragma` is not read (section 5.4 deprecates it).
pub(super) fn forbids_reuse(request: &HeaderMap) -> bool {
    directives(request).iter().any(|value| value == "no-cache")
}

/// How long a response may be reused.
///
/// `s-maxage` wins over `max-age`, this being a shared cache. No RFC 9111
/// section 4.2.2 heuristic; only a configured default.
fn freshness(control: &[String], default: Option<Duration>) -> Option<Duration> {
    for directive in ["s-maxage=", "max-age="] {
        if let Some(seconds) = control
            .iter()
            .find_map(|value| value.strip_prefix(directive))
            .and_then(|seconds| seconds.trim().parse::<u64>().ok())
        {
            return Some(Duration::from_secs(seconds));
        }
    }

    default
}

/// Every `Cache-Control` directive, lowercased.
fn directives(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(header::CACHE_CONTROL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|directive| directive.trim().to_ascii_lowercase())
        .filter(|directive| !directive.is_empty())
        .collect()
}

/// The field names a response varied on, lowercased and sorted.
pub(super) fn vary(headers: &HeaderMap) -> Vec<String> {
    let mut names: Vec<String> = headers
        .get_all(header::VARY)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();

    names.sort_unstable();
    names.dedup();
    names
}

/// Removes the fields a stored response must not keep.
pub(super) fn strip(headers: &mut HeaderMap) {
    for name in HOP_BY_HOP {
        headers.remove(*name);
    }
}
