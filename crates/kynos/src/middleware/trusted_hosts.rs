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
/// A request stating no authority — none on its target, and a `Host` absent
/// or empty, as RFC 9112 section 3.2 has a client send for a target with
/// none — is refused, since nothing on it names a host to admit, unless
/// [`admit_no_authority`](TrustedHosts::admit_no_authority) says otherwise.
/// A `TrustedHosts` with nothing allowed refuses every other request.
///
/// # Deciding elsewhere
///
/// [`check`](TrustedHosts::check) makes the interceptor's decision over a
/// request head and names why it refused, for an application that must make
/// it where no interceptor reaches, such as an authenticator; and
/// [`when`](TrustedHosts::when) mounts it for the deployments the context
/// says need it.
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
    /// Whether a request stating no authority is admitted.
    no_authority: bool,
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
            no_authority: self.no_authority,
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

    /// Also admits a request that states no authority: none on its target,
    /// and a `Host` absent or empty.
    ///
    /// No browser sends one, so a rebinding page cannot; a non-browser client
    /// that names no host, such as a local command-line tool, then reaches the
    /// service. A request naming a host is decided as before.
    #[must_use]
    pub fn admit_no_authority(mut self) -> Self {
        self.no_authority = true;
        self
    }

    /// Decides only where `enabled` says so of the context, and declares its
    /// 421 either way.
    ///
    /// For a deployment that is reachable by a rebinding page in one posture
    /// and not in another, decided at run time: the document describes what an
    /// operation *may* answer in any deployment, so the 421 stays declared
    /// where the check is off.
    ///
    /// ```
    /// use kynos::middleware::trusted_hosts::TrustedHosts;
    ///
    /// struct App {
    ///     /// Requests carry a bearer token, which a rebinding page lacks.
    ///     token_required: bool,
    /// }
    ///
    /// let hosts = TrustedHosts::new()
    ///     .allow_loopback()
    ///     .when(|app: &App| !app.token_required);
    /// # let _ = hosts;
    /// ```
    #[must_use]
    pub fn when<C>(self, enabled: fn(&C) -> bool) -> TrustedHostsWhen<C, T> {
        TrustedHostsWhen {
            hosts: self,
            enabled,
        }
    }

    /// Whether this configuration admits the request whose head is `parts`.
    ///
    /// The decision the interceptor makes, for an application that must make
    /// it somewhere an interceptor does not reach, such as inside an
    /// authenticator that alone knows whether the credential was ambient.
    /// [`check`](TrustedHosts::check) makes the same decision and names why it
    /// refused.
    #[must_use]
    pub fn admits(&self, parts: &http::Parts) -> bool {
        self.check(parts).is_ok()
    }

    /// The decision [`admits`](TrustedHosts::admits) makes, naming why it
    /// refused the request whose head is `parts`.
    ///
    /// ```
    /// use kynos::{
    ///     http::{HeaderValue, Request, body::Body, header},
    ///     middleware::trusted_hosts::{TrustedHosts, UntrustedHostReason},
    /// };
    ///
    /// let (mut parts, _) = Request::new(Body::empty()).into_parts();
    /// parts
    ///     .headers
    ///     .insert(header::HOST, HeaderValue::from_static("attacker.example"));
    ///
    /// let refused = TrustedHosts::new()
    ///     .allow_loopback()
    ///     .check(&parts)
    ///     .expect_err("a rebound request");
    /// assert_eq!(refused, UntrustedHostReason::NotAllowed);
    /// assert_eq!(refused.as_str(), "not_allowed");
    /// ```
    ///
    /// # Errors
    ///
    /// The [`UntrustedHostReason`] naming why the request was refused.
    pub fn check(&self, parts: &http::Parts) -> Result<(), UntrustedHostReason> {
        self.decides(&parts.headers, parts.uri.authority())
    }

    /// Whether a request with `headers`, whose target states `authority`,
    /// names a host this configuration admits, and why not where it does not.
    ///
    /// The target's authority wins where both are present, as it does for
    /// `Csrf` and `Cache`: RFC 9112 section 3.2.2 has a server ignore `Host`
    /// beside an absolute-form target, and RFC 9113 section 8.3.1 puts a
    /// version-2 request's `:authority` there.
    pub(crate) fn decides(
        &self,
        headers: &HeaderMap,
        authority: Option<&Authority>,
    ) -> Result<(), UntrustedHostReason> {
        let field;
        let host = if let Some(authority) = authority {
            authority.host()
        } else {
            let value = match headers.get(http::header::HOST) {
                Some(value) if !value.as_bytes().trim_ascii().is_empty() => value,
                // RFC 9112 section 3.2: an empty `Host` is a target with no
                // authority, as an absent one is.
                _ if self.no_authority => return Ok(()),
                _ => return Err(UntrustedHostReason::NoAuthority),
            };
            // `http`'s own authority grammar splits the port off, telling a
            // bracketed IPv6 literal's colons from the port's.
            field = Authority::try_from(value.as_bytes())
                .map_err(|_| UntrustedHostReason::Malformed)?;
            field.host()
        };
        let host = canonical(host).ok_or(UntrustedHostReason::Malformed)?;

        if (self.loopback && is_loopback(&host)) || self.hosts.contains(&host) {
            Ok(())
        } else {
            Err(UntrustedHostReason::NotAllowed)
        }
    }
}

/// Why [`TrustedHosts::check`] refused a request.
///
/// For a log line or a metric label. The refusal on the wire is one 421
/// whatever the reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UntrustedHostReason {
    /// The request stated no authority to admit: none on its target, and a
    /// `Host` absent or empty.
    NoAuthority,
    /// The authority stated is not one: `Host` does not parse as an
    /// authority, or names an empty host.
    Malformed,
    /// The authority names a host not on the list.
    NotAllowed,
}

impl UntrustedHostReason {
    /// A stable `snake_case` name, for a metric label or a structured log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoAuthority => "no_authority",
            Self::Malformed => "malformed",
            Self::NotAllowed => "not_allowed",
        }
    }
}

impl std::fmt::Display for UntrustedHostReason {
    /// A sentence naming the reason, for a human reading a log.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NoAuthority => "the request stated no authority",
            Self::Malformed => "the request's authority does not parse",
            Self::NotAllowed => "the request named a host this server does not answer for",
        })
    }
}

impl std::error::Error for UntrustedHostReason {}

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

    const READS_BODY: bool = false;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, UntrustedHost<T>> {
        let _ = (reads, context);

        if self
            .decides(request.headers(), request.uri().authority())
            .is_ok()
        {
            Ok(next.run(request).await)
        } else {
            Err(UntrustedHost::new())
        }
    }
}

/// A [`TrustedHosts`] that decides only where the context says so.
///
/// Built by [`TrustedHosts::when`]. Its `Short` is [`UntrustedHost`] whether
/// or not it is enabled, so every operation it covers declares the 421.
pub struct TrustedHostsWhen<C, T = ()> {
    hosts: TrustedHosts<T>,
    enabled: fn(&C) -> bool,
}

impl<C, T> Interceptor<C> for TrustedHostsWhen<C, T>
where
    C: Sync + 'static,
    T: ProblemType,
{
    type Reads = ();
    type Adds = ();
    type Short = UntrustedHost<T>;

    const READS_BODY: bool = false;

    async fn intercept(
        &self,
        request: http::Request,
        (): (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, UntrustedHost<T>> {
        if !(self.enabled)(context)
            || self
                .hosts
                .decides(request.headers(), request.uri().authority())
                .is_ok()
        {
            Ok(next.run(request).await)
        } else {
            Err(UntrustedHost::new())
        }
    }
}

// Not derived: a derive would bound each on `C` and the marker.
impl<C, T> Clone for TrustedHostsWhen<C, T> {
    fn clone(&self) -> Self {
        Self {
            hosts: self.hosts.clone(),
            enabled: self.enabled,
        }
    }
}

impl<C, T> std::fmt::Debug for TrustedHostsWhen<C, T> {
    /// The `TrustedHosts` it applies; the predicate is code, and prints as
    /// nothing.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrustedHostsWhen")
            .field("hosts", &self.hosts)
            .finish_non_exhaustive()
    }
}

// Not derived: a derive would bound each on the marker.
marker_traits!(UntrustedHost<T> { problem_type: _ }: new, Copy, Debug, PartialEq);
marker_traits!(TrustedHosts<T> { problem_type: _, hosts, loopback, no_authority }: Clone, Debug);

impl<T> Default for TrustedHosts<T> {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            loopback: false,
            no_authority: false,
            problem_type: PhantomData,
        }
    }
}

#[cfg(test)]
#[path = "trusted_hosts/tests.rs"]
mod tests;
