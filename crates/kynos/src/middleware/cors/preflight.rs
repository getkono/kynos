//! What one path answers a browser preflight with.
//!
//! Out-of-document by construction. A [`Preflight`] is assembled while the
//! service is built — after the description has been emitted — so there is no
//! point at which a `paths` key could be minted from one. That is the whole of
//! the claim [`super`]'s module documentation makes: a preflight is a browser
//! protocol detail, not an operation of the API.

use std::time::Duration;

use kynos_openapi::Method;

use super::CorsConfig;
use crate::{
    http::{self, HeaderValue, Request, Response, StatusCode, header},
    router::policy::FallbackPolicy,
};

/// The `Vary` a preflight answer carries.
///
/// Three fields rather than one: the answer depends on the origin that asked,
/// on the method it proposed, and on the headers it proposed — so a cache that
/// keyed on origin alone could hand a `PUT` preflight's answer to a `DELETE`.
const PREFLIGHT_VARIES: &[&str] = &[
    "origin",
    "access-control-request-method",
    "access-control-request-headers",
];

/// One `Cors` covering a path, and the methods on that path it covers.
///
/// A path can hold more than one: a `Group`'s stack is checked against the
/// router's and never against a sibling's, so two groups may cover one path
/// with a configuration each and still compile.
pub(crate) struct Scope {
    /// The configuration of the `Cors` covering these methods.
    config: CorsConfig,
    /// The methods on this path that this configuration actually covers, with
    /// the HEAD a covered GET answers where the path declares none, which is
    /// what a proposed method is matched against.
    covered: Vec<Method>,
    /// The methods this scope advertises: the ones it covers, unless
    /// `allow_methods` overrode them.
    advertised: Vec<Method>,
}

impl Scope {
    /// Records one configuration and the methods it covers.
    pub(crate) fn new(config: CorsConfig, covered: Vec<Method>) -> Self {
        // The override exists for a deployment fronting routes Kynos does not
        // serve; without it the advertised set is what this scope covers --
        // its declared methods and the HEAD each covered GET answers -- which
        // is always a subset of `Allow`.
        let advertised = config.methods.clone().unwrap_or_else(|| covered.clone());

        Self {
            config,
            covered,
            advertised,
        }
    }

    /// Whether this scope covers the method a preflight proposed.
    fn covers(&self, proposed: &HeaderValue) -> bool {
        names(&self.covered, proposed)
    }

    /// Whether this scope's `allow_methods` override names the method a
    /// preflight proposed.
    fn overrides_for(&self, proposed: &HeaderValue) -> bool {
        self.config
            .methods
            .as_deref()
            .is_some_and(|methods| names(methods, proposed))
    }
}

/// Whether `methods` names the method a preflight proposed.
fn names(methods: &[Method], proposed: &HeaderValue) -> bool {
    methods.iter().any(|method| {
        proposed
            .as_bytes()
            .eq_ignore_ascii_case(method.as_wire_str().as_bytes())
    })
}

/// What a path answers an `OPTIONS` request with, once CORS covers it.
pub(crate) struct Preflight {
    /// Every `Cors` covering this path, in mount order.
    scopes: Vec<Scope>,
    /// Every method an operation on this path answers, with the HEAD a GET
    /// answers where the path declares none. A proposed method in here that no
    /// scope covers runs under no `Cors`, so no override may approve it.
    served: Vec<Method>,
    /// The `Allow` header a non-preflight `OPTIONS` carries, so that request
    /// keeps the answer it had before CORS was mounted. `None` where nothing
    /// in the service implements `OPTIONS`, whose answer is a 501 instead.
    allow: Option<HeaderValue>,
    /// The body shape a non-preflight `OPTIONS` takes, which is the router's
    /// own method-not-allowed policy rather than a second one invented here.
    fallback: FallbackPolicy,
}

impl Preflight {
    /// Assembles the answer for one path.
    ///
    /// `scopes` is non-empty: a path with no CORS on it gets no `Preflight` at
    /// all.
    pub(crate) fn new(
        scopes: Vec<Scope>,
        served: Vec<Method>,
        allow: Option<HeaderValue>,
        fallback: FallbackPolicy,
    ) -> Self {
        debug_assert!(!scopes.is_empty(), "a preflight with nothing covering it");

        Self {
            scopes,
            served,
            allow,
            fallback,
        }
    }

    /// The scope that answers a preflight proposing `method`, or `None` where
    /// it is refused.
    ///
    /// The scope covering it, since that is the one whose real response will
    /// carry the headers this answer promises. A method the path serves under
    /// no `Cors` is refused even where an `allow_methods` override names it:
    /// approving it would send the browser on to a request whose side effect
    /// runs while its response carries no CORS header. A method the path does
    /// not serve at all is answered by the first scope whose override names it,
    /// which is the deployment fronting routes Kynos does not serve.
    fn scope_for(&self, method: &HeaderValue) -> Option<&Scope> {
        if let Some(covering) = self.scopes.iter().find(|scope| scope.covers(method)) {
            return Some(covering);
        }

        if names(&self.served, method) {
            return None;
        }

        self.scopes.iter().find(|scope| scope.overrides_for(method))
    }

    /// Answers `request`.
    ///
    /// Implements the Fetch standard's preflight in order: a request that is not
    /// a preflight falls through to exactly the 405 or 501 the dispatcher would
    /// have produced, a method no configuration answers for and an origin the
    /// covering configuration does not permit are both answered with no CORS
    /// header at all, and a permitted one gets the full set.
    pub(crate) fn answer(&self, request: &Request) -> Response {
        let headers = request.headers();

        // Not a preflight. `Origin` and `Access-Control-Request-Method` are
        // both required of one, so an `OPTIONS` missing either is an ordinary
        // request for a method this path does not declare — and it keeps the
        // answer it had before CORS was mounted, byte for byte.
        let (Some(origin), Some(requested_method)) = (
            headers.get(header::ORIGIN),
            headers.get(header::ACCESS_CONTROL_REQUEST_METHOD),
        ) else {
            return self.not_a_preflight();
        };

        let mut response = Response::new(crate::http::body::Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;

        // `Vary` rides on every answer, permitted or not: what a cache must not
        // do is reuse a refusal for a different origin or method either.
        crate::middleware::vary_on(response.headers_mut(), PREFLIGHT_VARIES);

        // The proposed method picks the configuration, because that is the
        // scope whose real response will carry — or withhold — the headers this
        // answer is a promise about. An absent header is how the protocol says
        // no, to a method as to an origin. Inventing a 403 would be a status no
        // description declares, for a request that is not an operation.
        let Some(scope) = self.scope_for(requested_method) else {
            return response;
        };
        let config = &scope.config;

        if !config.permits(origin) {
            return response;
        }

        let fields = response.headers_mut();

        // `*` only where credentials are off. The pair is refused while the
        // router is built, so this is a named-allow-list echo rather than a
        // fallback.
        let allowed = if config.any.origin && !config.credentials {
            HeaderValue::from_static("*")
        } else {
            origin.clone()
        };
        fields.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, allowed);

        if config.credentials {
            fields.insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }

        if let Some(methods) = advertised_methods(&scope.advertised) {
            fields.insert(header::ACCESS_CONTROL_ALLOW_METHODS, methods);
        }

        if let Some(allowed) = advertised_headers(config, headers) {
            fields.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, allowed);
        }

        if let Some(max_age) = config.max_age.as_ref().and_then(seconds) {
            fields.insert(header::ACCESS_CONTROL_MAX_AGE, max_age);
        }

        // No `Access-Control-Expose-Headers`. It is read from the *actual*
        // response; the CORS-preflight fetch reads the allowed methods, the
        // allowed headers and the cache lifetime, and nothing else. Sending one
        // here is a field no user agent consults, suggesting a coverage the
        // preflight cannot grant.

        response
    }

    /// The answer an `OPTIONS` that is not a preflight gets.
    ///
    /// Reuses the dispatcher's own refusal, policy and `Allow` value rather
    /// than reimplementing them, so mounting CORS changes nothing about it.
    fn not_a_preflight(&self) -> Response {
        crate::router::dispatch::method_refusal(self.allow.as_ref(), &self.fallback)
    }
}

/// `Access-Control-Allow-Methods`, as one field value.
fn advertised_methods(methods: &[Method]) -> Option<HeaderValue> {
    if methods.is_empty() {
        return None;
    }

    let joined = methods
        .iter()
        .map(|method| method.as_wire_str())
        .collect::<Vec<_>>()
        .join(", ");

    HeaderValue::from_str(&joined).ok()
}

/// `Access-Control-Allow-Headers`, as one field value.
///
/// Under `allow_any_header` this is the verbatim echo of what was asked for
/// when credentials are on — `*` is not a wildcard on a credentialed response,
/// so echoing is the only way to answer one at all — and `*, authorization`
/// when they are off.
///
/// The second name is not redundant. The Fetch Standard calls `Authorization` a
/// *CORS non-wildcard request-header name* and checks it unconditionally: "If
/// one of request's header list's names is a CORS non-wildcard request-header
/// name and is not a byte-case-insensitive match for an item in headerNames,
/// then return a network error." The wildcard covers every *other* unsafe
/// header, and only while credentials are off; this one it never covers, so `*`
/// alone fails every request carrying a bearer token.
fn advertised_headers(config: &CorsConfig, request: &http::HeaderMap) -> Option<HeaderValue> {
    if config.any.header {
        if config.credentials {
            return request.get(header::ACCESS_CONTROL_REQUEST_HEADERS).cloned();
        }

        return Some(HeaderValue::from_static("*, authorization"));
    }

    if config.headers.is_empty() {
        return None;
    }

    let joined = config
        .headers
        .iter()
        .map(std::borrow::Cow::as_ref)
        .collect::<Vec<_>>()
        .join(", ");

    HeaderValue::from_str(&joined).ok()
}

/// A cache lifetime as the whole seconds `Access-Control-Max-Age` carries.
///
/// Rounded *up*, because the field has no sub-second form and truncating one
/// renders `0` — which tells the browser not to cache the preflight at all,
/// which is the opposite of what a lifetime was configured for. `rate_limit`
/// rounds `Retry-After` up for the same reason: between two answers neither of
/// which is exact, the misleading one is the one to avoid.
///
/// An explicit zero stays zero. It is the one value that already says what it
/// means.
fn seconds(age: &Duration) -> Option<HeaderValue> {
    let whole = if age.subsec_nanos() > 0 {
        age.as_secs().saturating_add(1)
    } else {
        age.as_secs()
    };

    HeaderValue::from_str(&whole.to_string()).ok()
}

#[cfg(test)]
mod tests;
