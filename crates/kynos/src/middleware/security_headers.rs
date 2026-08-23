//! Response headers that instruct a browser rather than describe a payload.
//!
//! Every field here is defined by a browser specification and read by browsers
//! alone: a generated REST client does nothing with `X-Frame-Options`. So the
//! group declares them — two interceptors setting `Referrer-Policy` is still a
//! compile error — and describes none of them, which is the call `Compression`
//! already makes for `Content-Encoding` and `Cors` for the `Access-Control-*`
//! set.
//!
//! What is *not* here is as deliberate. There is no permissive constructor:
//! `SecurityHeaders::new()` sends the four fields that are safe for an API
//! under any configuration, and CSP and HSTS are calls a reviewer can see.

use std::{borrow::Cow, convert::Infallible};

use crate::{
    extract::params::header::HeaderParams,
    http::{self, HeaderName, HeaderValue},
    middleware::{Continued, Interceptor, Next},
};

/// Sends the browser-directed security fields.
///
/// ```
/// use kynos::middleware::security_headers::{ReferrerPolicy, SecurityHeaders};
///
/// let headers = SecurityHeaders::new()
///     .referrer_policy(ReferrerPolicy::NoReferrer)
///     .content_security_policy("default-src 'none'; frame-ancestors 'none'")
///     .strict_transport_security(std::time::Duration::from_secs(63_072_000));
/// # let _ = headers;
/// ```
///
/// # `Strict-Transport-Security` is conditional, and has to be
///
/// RFC 6797 section 7.2: an HSTS host "MUST NOT include the STS header field in
/// HTTP responses conveyed over non-secure transport". Kynos therefore sends it
/// only where the client's own connection is known to have been secure —
/// [`Connection::is_secure`](crate::extract::connection::Connection::is_secure)
/// when Kynos terminated TLS, or a trusted hop's `proto` when it did not.
///
/// Behind a TLS-terminating proxy that means
/// [`Router::trusted_proxies`](crate::Router::trusted_proxies) has to be set, or
/// the field is never sent. That is the honest failure: unset, Kynos cannot tell
/// a proxied HTTPS request from a plaintext one, and guessing would send the
/// field over exactly the transport the specification forbids.
#[derive(Clone, Debug, Default)]
pub struct SecurityHeaders {
    frame: Option<FrameOptions>,
    referrer: Option<ReferrerPolicy>,
    permissions: Option<Cow<'static, str>>,
    csp: Option<Cow<'static, str>>,
    hsts: Option<Hsts>,
    nosniff: bool,
}

/// What `Strict-Transport-Security` says, once it may be said.
#[derive(Clone, Copy, Debug)]
struct Hsts {
    max_age: std::time::Duration,
    subdomains: bool,
    preload: bool,
}

/// `X-Frame-Options`, per RFC 7034.
///
/// Largely superseded by CSP's `frame-ancestors`, and still honoured by every
/// browser, so both are offered and neither is implied by the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameOptions {
    /// No origin may frame this response.
    Deny,
    /// Only the same origin may.
    SameOrigin,
}

impl FrameOptions {
    /// The field value this renders as.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "DENY",
            Self::SameOrigin => "SAMEORIGIN",
        }
    }
}

/// `Referrer-Policy`, per the W3C Referrer Policy report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReferrerPolicy {
    /// Never send the field.
    NoReferrer,
    /// Send it only to the same origin.
    SameOrigin,
    /// Full URL same-origin, origin only cross-origin, nothing on a downgrade.
    StrictOriginWhenCrossOrigin,
    /// Origin only, and nothing on a downgrade.
    StrictOrigin,
}

impl ReferrerPolicy {
    /// The field value this renders as.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoReferrer => "no-referrer",
            Self::SameOrigin => "same-origin",
            Self::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
            Self::StrictOrigin => "strict-origin",
        }
    }
}

impl SecurityHeaders {
    /// The fields that are right for an API under any configuration.
    ///
    /// `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY` and
    /// `Referrer-Policy: no-referrer`. An API serves no framed document and has
    /// no referrer worth leaking, so all three are safe defaults rather than
    /// opinions — unlike CSP and HSTS, which are deployment decisions and are
    /// off until asked for.
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame: Some(FrameOptions::Deny),
            referrer: Some(ReferrerPolicy::NoReferrer),
            nosniff: true,
            ..Self::default()
        }
    }

    /// Sends nothing until told to. The base for a hand-built set.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Sets `X-Frame-Options`.
    #[must_use]
    pub fn frame_options(mut self, options: FrameOptions) -> Self {
        self.frame = Some(options);
        self
    }

    /// Sets `Referrer-Policy`.
    #[must_use]
    pub fn referrer_policy(mut self, policy: ReferrerPolicy) -> Self {
        self.referrer = Some(policy);
        self
    }

    /// Sets `Permissions-Policy`.
    #[must_use]
    pub fn permissions_policy(mut self, policy: impl Into<Cow<'static, str>>) -> Self {
        self.permissions = Some(policy.into());
        self
    }

    /// Sets `Content-Security-Policy`.
    #[must_use]
    pub fn content_security_policy(mut self, policy: impl Into<Cow<'static, str>>) -> Self {
        self.csp = Some(policy.into());
        self
    }

    /// Sends `Strict-Transport-Security` with this `max-age`, over a secure
    /// transport only.
    #[must_use]
    pub fn strict_transport_security(mut self, max_age: std::time::Duration) -> Self {
        self.hsts = Some(Hsts {
            max_age,
            subdomains: false,
            preload: false,
        });
        self
    }

    /// Adds `includeSubDomains` to the transport-security field.
    #[must_use]
    pub fn include_subdomains(mut self) -> Self {
        if let Some(hsts) = self.hsts.as_mut() {
            hsts.subdomains = true;
        }
        self
    }

    /// Adds `preload` to the transport-security field.
    ///
    /// Submitting a host to a preload list is close to irreversible, so this is
    /// its own call rather than a flag on the one above.
    #[must_use]
    pub fn preload(mut self) -> Self {
        if let Some(hsts) = self.hsts.as_mut() {
            hsts.preload = true;
        }
        self
    }

    /// The fields this configuration sends for a request that was `secure`.
    fn fields(&self, secure: bool) -> Vec<(HeaderName, HeaderValue)> {
        let mut fields = Vec::new();

        if self.nosniff {
            fields.push((
                HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            ));
        }
        if let Some(frame) = self.frame {
            fields.push((
                HeaderName::from_static("x-frame-options"),
                HeaderValue::from_static(frame.as_str()),
            ));
        }
        if let Some(referrer) = self.referrer {
            fields.push((
                HeaderName::from_static("referrer-policy"),
                HeaderValue::from_static(referrer.as_str()),
            ));
        }
        if let Some(permissions) = self.permissions.as_deref() {
            if let Ok(value) = HeaderValue::from_str(permissions) {
                fields.push((HeaderName::from_static("permissions-policy"), value));
            }
        }
        if let Some(csp) = self.csp.as_deref() {
            if let Ok(value) = HeaderValue::from_str(csp) {
                fields.push((HeaderName::from_static("content-security-policy"), value));
            }
        }

        // RFC 6797 section 7.2: never over non-secure transport.
        if let Some(hsts) = self.hsts.filter(|_| secure) {
            let mut rendered = format!("max-age={}", hsts.max_age.as_secs());
            if hsts.subdomains {
                rendered.push_str("; includeSubDomains");
            }
            if hsts.preload {
                rendered.push_str("; preload");
            }
            if let Ok(value) = HeaderValue::from_str(&rendered) {
                fields.push((HeaderName::from_static("strict-transport-security"), value));
            }
        }

        fields
    }
}

/// The fields [`SecurityHeaders`] attaches.
///
/// `DESCRIBED` is `false`: every name here is defined by a browser
/// specification and read by browsers alone, so a generated client has no use
/// for it. Declaring them still makes a second interceptor touching one a
/// compile error, which is the half that is about correctness.
#[derive(Clone, Debug, Default)]
pub struct SecurityHeaderNames(Vec<(HeaderName, HeaderValue)>);

impl HeaderParams for SecurityHeaderNames {
    const NAMES: &'static [&'static str] = &[
        "content-security-policy",
        "permissions-policy",
        "referrer-policy",
        "strict-transport-security",
        "x-content-type-options",
        "x-frame-options",
    ];

    const DESCRIBED: bool = false;

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        self.0.clone()
    }
}

impl<C: Sync + 'static> Interceptor<C> for SecurityHeaders {
    type Reads = ();
    type Adds = SecurityHeaderNames;
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<SecurityHeaderNames>, Infallible> {
        let _ = (reads, context);

        // Kynos's own transport first, because it is observed rather than
        // claimed. A trusted hop's `proto` is the answer only when Kynos did
        // not terminate TLS itself, and it is `None` until the application has
        // said whose word to take.
        let secure = request
            .extensions()
            .get::<crate::extract::connection::Connection>()
            .is_some_and(crate::extract::connection::Connection::is_secure)
            || request
                .extensions()
                .get::<crate::http::forwarded::Forwarded>()
                .and_then(crate::http::forwarded::Forwarded::client_is_secure)
                .unwrap_or(false);

        let fields = self.fields(secure);

        Ok(next
            .run(request)
            .await
            .with_headers(SecurityHeaderNames(fields)))
    }
}

#[cfg(test)]
#[path = "security_headers/tests.rs"]
mod tests;
