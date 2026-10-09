//! What one path answers a browser preflight with.
//!
//! Out-of-document by construction: a [`Preflight`] is assembled after the
//! description has been emitted.

use std::time::Duration;

use kynos_openapi::Method;

use super::CorsConfig;
use crate::{
    http::{self, HeaderValue, Request, Response, StatusCode, header},
    router::policy::FallbackPolicy,
};

/// The `Vary` a preflight answer carries: the answer depends on the origin,
/// the proposed method and the proposed headers.
const PREFLIGHT_VARIES: &[&str] = &[
    "origin",
    "access-control-request-method",
    "access-control-request-headers",
];

/// One `Cors` covering a path, and the methods on that path it covers. Sibling
/// groups may each cover one path.
pub(crate) struct Scope {
    /// The configuration of the `Cors` covering these methods.
    config: CorsConfig,
    /// The methods this configuration covers, plus the HEAD a covered GET
    /// answers where the path declares none.
    covered: Vec<Method>,
    /// The methods this scope advertises: the ones it covers, unless
    /// `allow_methods` overrode them, less any the path serves under no `Cors`.
    advertised: Vec<Method>,
}

impl Scope {
    /// Records one configuration and the methods it covers.
    pub(crate) fn new(config: CorsConfig, covered: Vec<Method>) -> Self {
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
    /// answers where the path declares none.
    served: Vec<Method>,
    /// The `Allow` a non-preflight `OPTIONS` carries; `None` where nothing
    /// implements `OPTIONS`, which answers 501.
    allow: Option<HeaderValue>,
    /// The router's method-not-allowed policy, for a non-preflight `OPTIONS`.
    fallback: FallbackPolicy,
}

impl Preflight {
    /// Assembles the answer for one path; `scopes` is non-empty.
    pub(crate) fn new(
        mut scopes: Vec<Scope>,
        served: Vec<Method>,
        allow: Option<HeaderValue>,
        fallback: FallbackPolicy,
    ) -> Self {
        debug_assert!(!scopes.is_empty(), "a preflight with nothing covering it");

        // A browser caches every advertised method and later skips the
        // preflight, so never advertise one the path serves under no `Cors`.
        let uncovered: Vec<Method> = served
            .iter()
            .copied()
            .filter(|method| !scopes.iter().any(|scope| scope.covered.contains(method)))
            .collect();
        for scope in &mut scopes {
            scope
                .advertised
                .retain(|method| !uncovered.contains(method));
        }

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
    /// The covering scope first. A method served under no `Cors` is refused
    /// even if an override names it; an unserved one goes to the first scope
    /// whose override names it.
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
    /// A non-preflight gets the dispatcher's 405 or 501; an unanswered method
    /// or unpermitted origin gets no CORS header; a permitted one the full set.
    pub(crate) fn answer(&self, request: &Request) -> Response {
        let headers = request.headers();

        // Missing either header: not a preflight, so answered as without CORS.
        let (Some(origin), Some(requested_method)) = (
            headers.get(header::ORIGIN),
            headers.get(header::ACCESS_CONTROL_REQUEST_METHOD),
        ) else {
            return self.not_a_preflight();
        };

        let mut response = Response::new(crate::http::body::Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;

        // On every answer: a cached refusal must not be reused either.
        crate::middleware::vary_on(response.headers_mut(), PREFLIGHT_VARIES);

        // Refused by absent headers, not an undeclared 403.
        let Some(scope) = self.scope_for(requested_method) else {
            return response;
        };
        let config = &scope.config;

        if !config.permits(origin) {
            return response;
        }

        let fields = response.headers_mut();

        // `*` only where credentials are off; the pair is refused at build.
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

        // No `Access-Control-Expose-Headers`: only the actual response's is read.

        response
    }

    /// The dispatcher's own refusal, for an `OPTIONS` that is not a preflight.
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
/// Under `allow_any_header`: an echo of the request when credentialed (`*` is
/// literal there), else `*, authorization`, since Fetch makes `Authorization`
/// a non-wildcard request-header name that `*` never covers.
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
/// Rounded up, since truncating a sub-second lifetime to `0` would disable
/// caching; an explicit zero stays zero.
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
