//! Requests for a host this server does not answer for, refused.
//!
//! A service on loopback with no credentials is reachable by DNS rebinding: a
//! page at `attacker.example` re-resolves its own name to `127.0.0.1`, so its
//! requests reach the service as same-origin requests for
//! `attacker.example`. Nothing in `Origin` or `Sec-Fetch-Site` gives that
//! away; the authority does. RFC 9110 section 17.1 leaves the server to decide
//! which authorities it is authoritative for, and this is where it decides.

use std::{
    borrow::Cow,
    marker::PhantomData,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use ::http::uri::Authority;

use crate::{
    error::problem::{IntoProblem, Problem, ProblemType, refusal_problem, refusal_response},
    http::{self, HeaderMap},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, ShortCircuit},
    schema::registry::Registry,
};

/// Refuses a request whose authority — its target's, else its `Host` — names
/// a host not on the list.
///
/// ```
/// use kynos::middleware::trusted_hosts::TrustedHosts;
///
/// let hosts = TrustedHosts::new().allow("api.example.com").allow_loopback();
/// # let _ = hosts;
/// ```
///
/// # What is compared
///
/// The host alone. The port is not, since a rebinding page reaches the
/// service on the port it listens on and names it in the authority it sends.
/// Hosts compare ASCII case-insensitively, a trailing `.` ignored, and an IP
/// literal in its canonical form, so `[0:0::1]` is `[::1]`. An IPv6 host is
/// written bracketed, as it is in a `Host` field, or bare: `allow("::1")` is
/// `allow("[::1]")`.
///
/// There is no wildcard. A rebinding attacker needs only a name that resolves
/// to the service, and one under an allowed suffix is a name whoever takes the
/// subdomain over controls.
///
/// A request stating no authority, which only HTTP/1.0 may send, is refused:
/// nothing on it names a host to admit. A `TrustedHosts` with nothing allowed
/// refuses every request.
///
/// # Loopback
///
/// [`allow_loopback`](TrustedHosts::allow_loopback) admits `localhost`, every
/// name under `.localhost`, and every loopback address: `127.0.0.0/8`, `::1`,
/// and `127.0.0.0/8` mapped into IPv6. RFC 6761 section 6.3 has a resolver
/// answer every `.localhost` name with loopback itself, so no page can rebind
/// one. `0.0.0.0` is not loopback, and is not admitted.
///
/// # The refusal
///
/// 421 Misdirected Request, which RFC 9110 section 15.5.20 defines as a
/// request "directed at a server that is unable or unwilling to produce an
/// authoritative response for the target URI", and section 7.4 names as the
/// answer to one. [`problem_type`](TrustedHosts::problem_type) names the
/// problem type the 421 carries.
///
/// It trusts what reaches it, as `Csrf` does: behind a reverse proxy that
/// rewrites `Host`, list the name the proxy sends.
pub struct TrustedHosts<T = ()> {
    /// Each allowed host, in [`canonical`] form.
    hosts: Vec<Cow<'static, str>>,
    loopback: bool,
    /// Names the refusal's problem type without holding one.
    problem_type: PhantomData<fn() -> T>,
}

impl TrustedHosts<()> {
    /// Admits no host until one is allowed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Names the RFC 9457 problem type this refusal's 421 carries.
    ///
    /// Available only on a `TrustedHosts` that has not named one, so a chain
    /// states the type at most once.
    ///
    /// ```
    /// use kynos::{error::problem::ProblemType, middleware::trusted_hosts::TrustedHosts};
    ///
    /// struct UnknownHost;
    ///
    /// impl ProblemType for UnknownHost {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/unknown-host");
    /// }
    ///
    /// let hosts = TrustedHosts::new()
    ///     .allow("api.example.com")
    ///     .problem_type::<UnknownHost>();
    /// # let _ = hosts;
    /// ```
    #[must_use]
    pub fn problem_type<T: ProblemType>(self) -> TrustedHosts<T> {
        TrustedHosts {
            hosts: self.hosts,
            loopback: self.loopback,
            problem_type: PhantomData,
        }
    }
}

impl<T> TrustedHosts<T> {
    /// Also admits requests for `host`, on any port.
    #[must_use]
    pub fn allow(mut self, host: impl Into<Cow<'static, str>>) -> Self {
        let host = host.into();
        let canonical = match canonical(&host) {
            Some(canonical) if canonical != host => Some(canonical.into_owned()),
            // Already canonical, or naming no host and so matching none: the
            // caller's value is kept as it came, and `Debug` shows it.
            _ => None,
        };
        self.hosts.push(canonical.map_or(host, Cow::Owned));
        self
    }

    /// Also admits requests for `localhost`, any `.localhost` name, and any
    /// loopback address.
    #[must_use]
    pub fn allow_loopback(mut self) -> Self {
        self.loopback = true;
        self
    }

    /// Whether a request with `headers`, whose target states `authority`,
    /// names a host this configuration admits.
    ///
    /// The target's authority wins where both are present, as it does for
    /// `Csrf` and `Cache`: RFC 9112 section 3.2.2 has a server ignore `Host`
    /// beside an absolute-form target, and RFC 9113 section 8.3.1 puts a
    /// version-2 request's `:authority` there.
    pub(crate) fn admits(&self, headers: &HeaderMap, authority: Option<&Authority>) -> bool {
        let field;
        let host = if let Some(authority) = authority {
            authority.host()
        } else {
            // `http`'s own authority grammar splits the port off, telling a
            // bracketed IPv6 literal's colons from the port's.
            let Some(parsed) = headers
                .get(http::header::HOST)
                .and_then(|value| Authority::try_from(value.as_bytes()).ok())
            else {
                return false;
            };
            field = parsed;
            field.host()
        };
        let Some(host) = canonical(host) else {
            return false;
        };

        (self.loopback && is_loopback(&host)) || self.hosts.contains(&host)
    }
}

/// `host` as hosts are compared: lowercase, without a trailing `.`, and an IP
/// literal in its canonical text, an IPv6 one bracketed. `None` for an empty
/// host.
///
/// Borrowed where `host` already was.
fn canonical(host: &str) -> Option<Cow<'_, str>> {
    let host = host.trim();
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return None;
    }

    let unbracketed = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(address) = unbracketed.parse::<IpAddr>() {
        let text = match address {
            IpAddr::V4(address) => address.to_string(),
            IpAddr::V6(address) => format!("[{address}]"),
        };
        return Some(if text == host {
            Cow::Borrowed(host)
        } else {
            Cow::Owned(text)
        });
    }

    Some(if host.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(host.to_ascii_lowercase())
    } else {
        Cow::Borrowed(host)
    })
}

/// Whether a [`canonical`] host is `localhost`, a `.localhost` name, or a
/// loopback address.
fn is_loopback(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }

    let unbracketed = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'));
    match unbracketed {
        Some(inner) => inner.parse::<Ipv6Addr>().is_ok_and(|address| {
            address.is_loopback()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }),
        None => host
            .parse::<Ipv4Addr>()
            .is_ok_and(|address| address.is_loopback()),
    }
}

/// What a request for an untrusted host is answered with.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`TrustedHosts::problem_type`].
pub struct UntrustedHost<T = ()> {
    /// Carries `T` without storing one; `fn() -> T` keeps it `Send` and `Sync`.
    problem_type: PhantomData<fn() -> T>,
}

impl<T: ProblemType> IntoProblem for UntrustedHost<T> {
    fn into_problem(self) -> Problem {
        refusal_problem::<T>(http::StatusCode::MISDIRECTED_REQUEST)
            .with_detail("this server does not answer for the requested host")
    }

    fn statuses() -> &'static [http::StatusCode] {
        &[http::StatusCode::MISDIRECTED_REQUEST]
    }
}

impl<T: ProblemType> IntoResponse for UntrustedHost<T> {
    fn into_response(self) -> http::Response {
        self.into_problem().into_response()
    }
}

impl<T: ProblemType> ShortCircuit for UntrustedHost<T> {
    const STATUSES: &'static [u16] = &[421];
}

impl<T: ProblemType> crate::response::Responses for UntrustedHost<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            421,
            refusal_response::<T>(
                registry,
                421,
                "the request named a host this server does not answer for",
            ),
        )
    }
}

impl<C, T> Interceptor<C> for TrustedHosts<T>
where
    C: Sync + 'static,
    T: ProblemType,
{
    /// `()`: `Host` is the request's routing, not an operation parameter.
    type Reads = ();
    type Adds = ();
    type Short = UntrustedHost<T>;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, UntrustedHost<T>> {
        let _ = (reads, context);

        if self.admits(request.headers(), request.uri().authority()) {
            Ok(next.run(request).await)
        } else {
            Err(UntrustedHost::new())
        }
    }
}

// Not derived: a derive would bound each on the marker.
marker_traits!(UntrustedHost<T> { problem_type: _ }: new, Copy, Debug, PartialEq);
marker_traits!(TrustedHosts<T> { problem_type: _, hosts, loopback }: Clone, Debug);

impl<T> Default for TrustedHosts<T> {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            loopback: false,
            problem_type: PhantomData,
        }
    }
}

#[cfg(test)]
#[path = "trusted_hosts/tests.rs"]
mod tests;
