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
    response::IntoResponse,
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

/// What a path answers an `OPTIONS` request with, once CORS covers it.
pub(crate) struct Preflight {
    /// The configuration of the `Cors` covering this path.
    config: CorsConfig,
    /// The methods this answer advertises: the ones actually declared on the
    /// path, unless `allow_methods` overrode them.
    methods: Vec<Method>,
    /// The `Allow` header a non-preflight `OPTIONS` carries, so that request
    /// keeps the answer it had before CORS was mounted.
    allow: HeaderValue,
    /// The body shape a non-preflight `OPTIONS` takes, which is the router's
    /// own method-not-allowed policy rather than a second one invented here.
    fallback: FallbackPolicy,
}

impl Preflight {
    /// Assembles the answer for one path.
    pub(crate) fn new(
        config: CorsConfig,
        declared: &[Method],
        allow: HeaderValue,
        fallback: FallbackPolicy,
    ) -> Self {
        // The override exists for a deployment fronting routes Kynos does not
        // serve; without it the advertised set is what the path declares, so
        // preflight and the description cannot disagree.
        let methods = config.methods.clone().unwrap_or_else(|| declared.to_vec());

        Self {
            config,
            methods,
            allow,
            fallback,
        }
    }

    /// Answers `request`.
    ///
    /// Implements the Fetch standard's preflight in order: a request that is not
    /// a preflight falls through to exactly the 405 the dispatcher would have
    /// produced, an origin the configuration does not permit is answered with no
    /// CORS header at all, and a permitted one gets the full set.
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
        // do is reuse a refusal for a different origin either.
        crate::middleware::vary_on(response.headers_mut(), PREFLIGHT_VARIES);

        if !self.config.permits(origin) {
            // An absent header is how the protocol says no. Inventing a 403
            // would be a status no description declares, for a request that is
            // not an operation.
            return response;
        }

        let fields = response.headers_mut();

        // `*` only where credentials are off. The pair is refused while the
        // router is built, so this is a named-allow-list echo rather than a
        // fallback.
        let allowed = if self.config.any_origin && !self.config.credentials {
            HeaderValue::from_static("*")
        } else {
            origin.clone()
        };
        fields.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, allowed);

        if self.config.credentials {
            fields.insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }

        if let Some(methods) = self.advertised_methods() {
            fields.insert(header::ACCESS_CONTROL_ALLOW_METHODS, methods);
        }

        if let Some(allowed) = self.advertised_headers(requested_method, headers) {
            fields.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, allowed);
        }

        if let Some(max_age) = self.config.max_age.as_ref().and_then(seconds) {
            fields.insert(header::ACCESS_CONTROL_MAX_AGE, max_age);
        }

        if let Some(expose) = self.config.exposed() {
            fields.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, expose);
        }

        response
    }

    /// The answer an `OPTIONS` that is not a preflight gets.
    ///
    /// Reuses the dispatcher's own policy and `Allow` value rather than
    /// reimplementing them, so mounting CORS changes nothing about it.
    fn not_a_preflight(&self) -> Response {
        let mut response = match self.fallback {
            FallbackPolicy::Problem => {
                crate::error::problem::Problem::new(StatusCode::METHOD_NOT_ALLOWED).into_response()
            }
            FallbackPolicy::Empty => {
                let mut response = Response::new(crate::http::body::Body::empty());
                *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
                response
            }
        };

        response
            .headers_mut()
            .insert(header::ALLOW, self.allow.clone());

        response
    }

    /// `Access-Control-Allow-Methods`, as one field value.
    fn advertised_methods(&self) -> Option<HeaderValue> {
        if self.methods.is_empty() {
            return None;
        }

        let joined = self
            .methods
            .iter()
            .map(|method| method.as_wire_str())
            .collect::<Vec<_>>()
            .join(", ");

        HeaderValue::from_str(&joined).ok()
    }

    /// `Access-Control-Allow-Headers`, as one field value.
    ///
    /// Under `allow_any_header` this is `*` when credentials are off, and the
    /// verbatim echo of what was asked for when they are on — `*` is not a
    /// wildcard on a credentialed response, so echoing is the only way to
    /// answer one at all.
    fn advertised_headers(
        &self,
        _requested_method: &HeaderValue,
        request: &http::HeaderMap,
    ) -> Option<HeaderValue> {
        if self.config.any_header {
            if self.config.credentials {
                return request.get(header::ACCESS_CONTROL_REQUEST_HEADERS).cloned();
            }

            return Some(HeaderValue::from_static("*"));
        }

        if self.config.headers.is_empty() {
            return None;
        }

        let joined = self
            .config
            .headers
            .iter()
            .map(std::borrow::Cow::as_ref)
            .collect::<Vec<_>>()
            .join(", ");

        HeaderValue::from_str(&joined).ok()
    }
}

/// A cache lifetime as the whole seconds `Access-Control-Max-Age` carries.
fn seconds(age: &Duration) -> Option<HeaderValue> {
    HeaderValue::from_str(&age.as_secs().to_string()).ok()
}

#[cfg(test)]
#[path = "preflight_tests.rs"]
mod tests;
