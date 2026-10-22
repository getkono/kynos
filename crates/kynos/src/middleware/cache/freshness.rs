//! What a shared cache may store, and for how long.

use std::time::{Duration, SystemTime};

use crate::http::{HeaderMap, StatusCode, date, header};

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

/// What the rules read from a request's own fields, taken before the request is
/// handed on so that its map is not kept across the handler.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Requested {
    /// It said `no-cache`.
    no_cache: bool,
    /// It said `no-store`.
    no_store: bool,
    /// It carried `Authorization`.
    authorized: bool,
}

impl Requested {
    /// Reads `request`'s `Cache-Control` once, and whether it carries
    /// `Authorization`.
    pub(super) fn of(request: &HeaderMap) -> Self {
        let mut requested = Self {
            authorized: request.contains_key(header::AUTHORIZATION),
            ..Self::default()
        };
        for directive in directives(request) {
            match directive.as_str() {
                "no-cache" => requested.no_cache = true,
                "no-store" => requested.no_store = true,
                _ => {}
            }
        }
        requested
    }

    /// Whether the request forbids answering it from the store.
    ///
    /// RFC 9111 section 5.2.1.4 `no-cache`, since this cache does not
    /// validate. `Pragma` is not read (section 5.4 deprecates it).
    pub(super) fn forbids_reuse(self) -> bool {
        self.no_cache
    }

    /// Whether the request forbids storing its response (RFC 9111 section
    /// 5.2.1.5 `no-store`).
    pub(super) fn forbids_storing(self) -> bool {
        self.no_store
    }
}

/// Whether a response to `method` may be stored at all.
///
/// RFC 9111 section 3 permits caching `POST` only with `Content-Location`;
/// not supported.
pub(super) fn storable_method(method: &crate::http::Method) -> bool {
    matches!(
        method,
        &crate::http::Method::GET | &crate::http::Method::HEAD
    )
}

/// Whether a response may be stored, and for how long.
///
/// `secured` is whether the operation declares a security requirement, as
/// routing recorded it for the request.
pub(super) fn storable(
    method: &crate::http::Method,
    status: StatusCode,
    request: Requested,
    response: &HeaderMap,
    secured: bool,
    default_freshness: Option<Duration>,
) -> Result<Duration, Unstorable> {
    if !storable_method(method) {
        return Err(Unstorable::Method);
    }

    if !CACHEABLE.contains(&status.as_u16()) {
        return Err(Unstorable::Status);
    }

    if request.no_store {
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
    if (secured || request.authorized) && !shared(&response_control) {
        return Err(Unstorable::Authorized);
    }

    freshness(&response_control, response, default_freshness).ok_or(Unstorable::NoFreshness)
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

/// The lifetime RFC 9111 section 1.2.2 has a cache read a `delta-seconds` too
/// large to represent as.
const SATURATED: u64 = 1 << 31;

/// How long a response may be reused.
///
/// RFC 9111 section 4.2.1's order: `s-maxage`, this being a shared cache, then
/// `max-age`, then `Expires` minus `Date`. The first of them present decides,
/// and where it is unreadable or repeated the response is stale (zero), as
/// that section encourages: the default is for a response that stated no
/// lifetime, not one that stated a bad one. No section 4.2.2 heuristic; only a
/// configured default.
fn freshness(
    control: &[String],
    response: &HeaderMap,
    default: Option<Duration>,
) -> Option<Duration> {
    for name in ["s-maxage", "max-age"] {
        let mut stated = control.iter().filter_map(|value| argument(value, name));
        if let Some(first) = stated.next() {
            let seconds = match stated.next() {
                None => delta_seconds(first).unwrap_or(0),
                Some(_) => 0,
            };
            return Some(Duration::from_secs(seconds));
        }
    }

    let mut expires = response.get_all(header::EXPIRES).iter();
    if let Some(first) = expires.next() {
        // Section 5.3: an unreadable `Expires` is already past.
        let Some(expiry) = expires
            .next()
            .is_none()
            .then(|| first.to_str().ok().and_then(date::parse))
            .flatten()
        else {
            return Some(Duration::ZERO);
        };

        // Without a readable `Date`, the time it was received.
        let mut dates = response.get_all(header::DATE).iter();
        let origin_now = match (dates.next(), dates.next()) {
            (Some(value), None) => value.to_str().ok().and_then(date::parse),
            _ => None,
        }
        .unwrap_or_else(SystemTime::now);

        return Some(expiry.duration_since(origin_now).unwrap_or_default());
    }

    default
}

/// The argument `directive` gives `name`: empty where it has none, and `None`
/// where `directive` is another one.
fn argument<'a>(directive: &'a str, name: &str) -> Option<&'a str> {
    let rest = directive.strip_prefix(name)?;
    if rest.is_empty() {
        Some(rest)
    } else {
        rest.strip_prefix('=')
    }
}

/// A `delta-seconds` argument, in the token form or the quoted-string form
/// section 5.2 has a recipient accept; [`SATURATED`] past `u64`.
fn delta_seconds(argument: &str) -> Option<u64> {
    let digits = argument
        .strip_prefix('"')
        .and_then(|quoted| quoted.strip_suffix('"'))
        .unwrap_or(argument);

    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| digits.parse().unwrap_or(SATURATED))
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
