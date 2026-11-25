//! Baseline response security headers.
//!
//! [`SecurityHeaders`] sets the fields an API response should carry whatever
//! the operation: `Cache-Control: no-store` (RFC 9111 section 5.2.2.5),
//! `Referrer-Policy: no-referrer` (W3C Referrer Policy sections 3.1 and 4.1)
//! and `X-Content-Type-Options: nosniff` (WHATWG Fetch section 3.6).
//! `Strict-Transport-Security` (RFC 6797 section 6.1) and
//! `X-Frame-Options: DENY` (RFC 7034 section 2.1) are opt-in, and each changes
//! the declared group.

use std::{convert::Infallible, marker::PhantomData, time::Duration};

use kynos_openapi::{
    Header, Map, MediaType, RefOr, Schema, SchemaObject,
    model::{
        body::mime_names,
        schema::types::{SchemaType, TypeSet},
    },
};
use serde_json::Value;

use crate::{
    extract::{
        connection::Connection,
        params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    },
    http::{self, HeaderName, HeaderValue, forwarded::Forwarded, header},
    middleware::{Continued, Interceptor, Next},
    schema::registry::Registry,
};

const NO_STORE: &str = "no-store";
const NO_REFERRER: &str = "no-referrer";
const NOSNIFF: &str = "nosniff";
const DENY: &str = "DENY";

/// Sets the baseline security headers on every response it covers.
///
/// Each field is inserted, replacing whatever value the chain produced, so the
/// `const` every covered operation documents is the value sent. An operation
/// that wants to be stored by its clients does not belong under it.
///
/// The two type parameters record the opt-ins, since each adds a declared
/// field: mounting it beside anything else setting one of them is a compile
/// error. Each is a marker named for what it records, [`Hsts`] or [`NoHsts`]
/// first and [`DenyFraming`] or [`NoDenyFraming`] second, so a function
/// returning one names it as it reads:
///
/// ```
/// use std::time::Duration;
///
/// use kynos::middleware::security_headers::{
///     DenyFraming, Hsts, SecurityHeaders, StrictTransportSecurity,
/// };
///
/// fn headers() -> SecurityHeaders<Hsts, DenyFraming> {
///     SecurityHeaders::new()
///         .strict_transport_security(
///             StrictTransportSecurity::max_age(Duration::from_secs(31_536_000))
///                 .include_subdomains(),
///         )
///         .deny_framing()
/// }
/// # let _ = headers();
/// ```
///
/// The two positions take different traits, so the markers cannot be swapped:
///
/// ```compile_fail
/// use kynos::middleware::security_headers::{DenyFraming, NoHsts, SecurityHeaders};
///
/// fn mount(headers: SecurityHeaders<DenyFraming, NoHsts>) {}
/// ```
#[derive(Clone, Debug)]
pub struct SecurityHeaders<H: HstsSetting = NoHsts, F: FramingSetting = NoDenyFraming> {
    /// The rendered `Strict-Transport-Security` value; `Some` exactly when
    /// `H` is [`Hsts`].
    transport: Option<HeaderValue>,
    _settings: PhantomData<fn() -> (H, F)>,
}

mod private {
    /// What a setting marker records, readable in a `const`.
    pub trait Setting {
        /// Whether the field the marker's position governs is sent.
        const SENT: bool;
    }
}

/// Whether a [`SecurityHeaders`] sends `Strict-Transport-Security`: [`Hsts`]
/// or [`NoHsts`].
///
/// This trait is sealed; the two markers are its only implementations.
pub trait HstsSetting: private::Setting + Send + Sync + 'static {}

/// Whether a [`SecurityHeaders`] sends `X-Frame-Options: DENY`:
/// [`DenyFraming`] or [`NoDenyFraming`].
///
/// This trait is sealed; the two markers are its only implementations.
pub trait FramingSetting: private::Setting + Send + Sync + 'static {}

/// Sends `Strict-Transport-Security`, as
/// [`strict_transport_security`](SecurityHeaders::strict_transport_security)
/// asked.
#[derive(Clone, Copy, Debug)]
pub enum Hsts {}

/// Sends no `Strict-Transport-Security`, the default.
#[derive(Clone, Copy, Debug)]
pub enum NoHsts {}

/// Sends `X-Frame-Options: DENY`, as
/// [`deny_framing`](SecurityHeaders::deny_framing) asked.
#[derive(Clone, Copy, Debug)]
pub enum DenyFraming {}

/// Sends no `X-Frame-Options`, the default.
#[derive(Clone, Copy, Debug)]
pub enum NoDenyFraming {}

impl private::Setting for Hsts {
    const SENT: bool = true;
}
impl private::Setting for NoHsts {
    const SENT: bool = false;
}
impl private::Setting for DenyFraming {
    const SENT: bool = true;
}
impl private::Setting for NoDenyFraming {
    const SENT: bool = false;
}
impl HstsSetting for Hsts {}
impl HstsSetting for NoHsts {}
impl FramingSetting for DenyFraming {}
impl FramingSetting for NoDenyFraming {}

impl Default for SecurityHeaders {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityHeaders {
    /// `Cache-Control`, `Referrer-Policy` and `X-Content-Type-Options`, and
    /// nothing opted into.
    #[must_use]
    pub fn new() -> Self {
        Self::with(None)
    }
}

impl<H: HstsSetting, F: FramingSetting> SecurityHeaders<H, F> {
    /// The interceptor at any settings, sending `transport` as its HSTS value.
    fn with(transport: Option<HeaderValue>) -> Self {
        Self {
            transport,
            _settings: PhantomData,
        }
    }

    /// Also sends `Strict-Transport-Security`, replacing any policy set before.
    ///
    /// Sent only on a response to a request conveyed over a secure transport,
    /// since RFC 6797 section 7.2 forbids it otherwise: a trusted hop's
    /// [`Forwarded`] scheme where one was stated, else whether the socket the
    /// client connected on completed a TLS handshake. Behind a proxy that
    /// terminates TLS, configure
    /// [`Router::trusted_proxies`](crate::Router::trusted_proxies) or it is
    /// never sent. Where it is withheld, a policy the chain set is removed.
    #[must_use]
    pub fn strict_transport_security(
        self,
        policy: StrictTransportSecurity,
    ) -> SecurityHeaders<Hsts, F> {
        SecurityHeaders::with(Some(policy.value()))
    }

    /// Also sends `X-Frame-Options: DENY`, refusing every framing of the
    /// response.
    #[must_use]
    pub fn deny_framing(self) -> SecurityHeaders<H, DenyFraming> {
        SecurityHeaders::with(self.transport)
    }
}

/// A `Strict-Transport-Security` policy (RFC 6797 section 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrictTransportSecurity {
    max_age: Duration,
    include_subdomains: bool,
}

impl StrictTransportSecurity {
    /// Known as an HSTS host for `max_age`, in whole seconds rounded down.
    ///
    /// Zero tells a client to forget the host (section 6.1.1).
    #[must_use]
    pub fn max_age(max_age: Duration) -> Self {
        Self {
            max_age,
            include_subdomains: false,
        }
    }

    /// Also covers every subdomain of the host (section 6.1.2).
    #[must_use]
    pub fn include_subdomains(mut self) -> Self {
        self.include_subdomains = true;
        self
    }

    /// The field value, directives in the order section 6.2 writes them.
    pub(crate) fn value(&self) -> HeaderValue {
        let seconds = self.max_age.as_secs();
        let value = if self.include_subdomains {
            format!("max-age={seconds}; includeSubDomains")
        } else {
            format!("max-age={seconds}")
        };

        HeaderValue::try_from(value).expect("digits and directive names are a valid field value")
    }
}

/// The group [`SecurityHeaders`] declares and attaches.
///
/// Built only by the interceptor; the parameters mirror its own.
#[derive(Clone, Debug)]
pub struct SecurityFields<H: HstsSetting, F: FramingSetting> {
    /// `Strict-Transport-Security`, where this response may carry it.
    transport: Option<HeaderValue>,
    _settings: PhantomData<fn() -> (H, F)>,
}

impl<H: HstsSetting, F: FramingSetting> SecurityFields<H, F> {
    /// The group carrying `transport` as its HSTS value.
    pub(crate) fn new(transport: Option<HeaderValue>) -> Self {
        Self {
            transport,
            _settings: PhantomData,
        }
    }
}

impl<H: HstsSetting, F: FramingSetting> HeaderParams for SecurityFields<H, F> {
    const NAMES: &'static [&'static str] = match (H::SENT, F::SENT) {
        (false, false) => &["cache-control", "referrer-policy", "x-content-type-options"],
        (true, false) => &[
            "cache-control",
            "referrer-policy",
            "x-content-type-options",
            "strict-transport-security",
        ],
        (false, true) => &[
            "cache-control",
            "referrer-policy",
            "x-content-type-options",
            "x-frame-options",
        ],
        (true, true) => &[
            "cache-control",
            "referrer-policy",
            "x-content-type-options",
            "strict-transport-security",
            "x-frame-options",
        ],
    };

    fn response_headers(registry: &mut Registry) -> Map<RefOr<Header>> {
        let _ = registry;
        let mut headers = Map::new();

        let mut fixed = |name: &str, value: &str, description: &str| {
            headers.insert(
                name.to_owned(),
                RefOr::Item(
                    text(constant(value))
                        .with_description(description)
                        .required(true),
                ),
            );
        };

        fixed(
            "Cache-Control",
            NO_STORE,
            "No cache may store this response, per RFC 9111 section 5.2.2.5",
        );
        fixed(
            "Referrer-Policy",
            NO_REFERRER,
            "A request this response causes carries no `Referer`",
        );
        fixed(
            "X-Content-Type-Options",
            NOSNIFF,
            "The response is not to be read as any type but its `Content-Type`",
        );

        if F::SENT {
            fixed(
                "X-Frame-Options",
                DENY,
                "The response is not to be rendered in a frame",
            );
        }

        if H::SENT {
            headers.insert(
                "Strict-Transport-Security".to_owned(),
                RefOr::Item(text(Schema::of_type(SchemaType::String)).with_description(
                    "The host is to be reached over HTTPS only, per RFC 6797. Sent only on a \
                         response to a request conveyed over a secure transport",
                )),
            );
        }

        headers
    }
}

impl<H: HstsSetting, F: FramingSetting> EncodeHeaders for SecurityFields<H, F> {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        let mut fields = vec![
            (header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE)),
            (
                header::REFERRER_POLICY,
                HeaderValue::from_static(NO_REFERRER),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static(NOSNIFF),
            ),
        ];

        if let Some(transport) = &self.transport {
            fields.push((header::STRICT_TRANSPORT_SECURITY, transport.clone()));
        }

        if F::SENT {
            fields.push((header::X_FRAME_OPTIONS, HeaderValue::from_static(DENY)));
        }

        Ok(fields)
    }
}

/// A header described as text, since an HSTS value's `;` and `=` would be
/// percent-encoded under a `schema`'s `simple` style (OpenAPI 3.2 Appendix D).
fn text(schema: Schema) -> Header {
    Header::with_content(mime_names::TEXT_PLAIN, MediaType::new(schema))
}

/// The schema of a string field that only ever carries `value`.
fn constant(value: &str) -> Schema {
    Schema::Object(Box::new(SchemaObject {
        ty: Some(TypeSet::One(SchemaType::String)),
        const_value: Some(Value::String(value.to_owned())),
        ..SchemaObject::default()
    }))
}

/// Whether the client's own connection was secure, as RFC 6797 section 7.2
/// asks.
///
/// A scheme a trusted hop stated decides. Failing one, the socket's TLS counts
/// only where the socket peer is the resolved client, since otherwise it
/// describes a proxy's connection rather than the client's. An IPv4-mapped
/// peer is compared in its IPv4 form, as [`Forwarded::client`] names it.
pub(crate) fn conveyed_securely(
    forwarded: Option<&Forwarded>,
    connection: Option<&Connection>,
) -> bool {
    if let Some(secure) = forwarded.and_then(Forwarded::client_is_secure) {
        return secure;
    }

    connection.is_some_and(|connection| {
        connection.is_tls()
            && forwarded.is_none_or(|forwarded| {
                connection
                    .peer_addr()
                    .is_some_and(|peer| forwarded.client() == Some(peer.ip().to_canonical()))
            })
    })
}

impl<C, H: HstsSetting, F: FramingSetting> Interceptor<C> for SecurityHeaders<H, F>
where
    C: Sync + 'static,
{
    type Reads = ();
    type Adds = SecurityFields<H, F>;

    /// Always continues: the fields go on whatever the chain returns.
    type Short = Infallible;

    const READS_BODY: bool = false;

    // Reads the request before building the future, so the future holds the
    // rest of the chain rather than the request.
    fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<Self::Adds>, Infallible>> + Send {
        let _ = (reads, context);

        let transport = self.transport.clone().filter(|_| {
            conveyed_securely(
                Forwarded::of(&request),
                request.extensions().get::<Connection>(),
            )
        });

        let withheld = H::SENT && transport.is_none();
        let run = next.run(request);

        async move {
            let mut continued = run.await.with_headers(SecurityFields::new(transport));

            // A policy the chain set is replaced like every other field, and
            // where none may be sent, replaced by nothing.
            if withheld {
                continued.remove_declared::<Self::Adds>(&header::STRICT_TRANSPORT_SECURITY);
            }

            Ok(continued)
        }
    }
}
