//! What a shared cache may store, and for how long.

use std::time::Duration;

use crate::http::{HeaderMap, StatusCode, header};

/// Statuses a response may be stored under.
///
/// RFC 9110 section 15.1's heuristically-cacheable set, minus 206. A partial
/// response *can* arise — [`response::range`](crate::response::range) serves
/// one — and 206 stays out because this cache stores and replays whole
/// responses: it has no way to recombine a stored part with the range a later
/// request asks for, and section 14.4 forbids recombining what a recipient
/// cannot verify. A closed enumeration, checked by a table test.
pub(super) const CACHEABLE: &[u16] = &[200, 203, 204, 300, 301, 308, 404, 405, 410, 414, 501];

/// Fields a stored response must not keep.
///
/// RFC 9110 section 7.6.1: connection-specific, and meaningless to whoever
/// reads the response back. `Age` goes with them because it is recomputed on
/// the way out — a stored one would be the age at the time of storage, added to
/// the age since.
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

/// Why a response was not stored.
///
/// Not public: an application does not act on it. Named rather than a `bool`
/// so a reader of the code can see the whole list at once, and so the table
/// test can count its cases against the set.
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
    /// The request carried credentials and the response did not say it was
    /// shareable.
    Authorized,
    /// The response said nothing about how long it may be reused, and no
    /// default was configured.
    NoFreshness,
    /// The body's length is unknown, or past the configured maximum.
    Body,
}

/// Whether a response may be stored, and for how long.
pub(super) fn storable(
    method: &crate::http::Method,
    status: StatusCode,
    request: &HeaderMap,
    response: &HeaderMap,
    default_freshness: Option<Duration>,
) -> Result<Duration, Unstorable> {
    // RFC 9111 section 3 permits `POST` only with an explicit
    // `Content-Location`, and getting that wrong is a correctness bug for a
    // capability nobody asks for.
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
        // `private` and `no-cache` may name fields, which narrows them. Kynos
        // treats a narrowed directive as the whole one: storing part of a
        // response is not something this cache can do, so the conservative
        // reading is the only correct one.
        if response_control
            .iter()
            .any(|value| value == directive || value.starts_with(&format!("{directive}=")))
        {
            return Err(refusal);
        }
    }

    if response
        .get(header::VARY)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|name| name.trim() == "*"))
    {
        return Err(Unstorable::VaryWildcard);
    }

    // No opt-out. Replaying a response that mints a session to a second client
    // is the worst bug a cache has, and `Vary` cannot protect against it: the
    // cookie is in the *response*, and nothing in the request selects it.
    if response.contains_key(header::SET_COOKIE) {
        return Err(Unstorable::SetCookie);
    }

    // RFC 9111 section 3.5: a response to an authenticated request is shared
    // only where it says so.
    if request.contains_key(header::AUTHORIZATION)
        && !response_control
            .iter()
            .any(|value| value == "public" || value.starts_with("s-maxage="))
    {
        return Err(Unstorable::Authorized);
    }

    freshness(&response_control, default_freshness).ok_or(Unstorable::NoFreshness)
}

/// How long a response may be reused.
///
/// `s-maxage` wins over `max-age`, because this is a shared cache and that is
/// what the directive is for.
///
/// There is no heuristic. RFC 9111 section 4.2.2 permits one, and every
/// heuristic is a guess that turns a correct origin into an incorrect cache —
/// so a response that did not say is not reused unless a default was configured
/// deliberately.
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
