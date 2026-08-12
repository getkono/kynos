//! Correlation identifiers.

use std::marker::PhantomData;

use std::convert::Infallible;

use crate::{
    extract::params::header::HeaderParams,
    http,
    middleware::{Continued, Interceptor, Next},
};

/// Supplies identifiers for requests that arrive without one.
///
/// Kynos owns the header and the contribution; the identifier *format* stays
/// the application's, because prescribing one would mean prescribing a UUID or
/// trace-context dependency that most applications already have their own
/// opinion about.
pub trait RequestIdSource: Send + Sync + 'static {
    /// Produces an identifier for a request that carried none.
    fn next_id(&self) -> http::HeaderValue;
}

/// A dependency-free source: a per-process counter.
///
/// Unique within one process and no further. Enough to correlate a request
/// across its own logs, which is what the default is for; reach for a real
/// identifier scheme when correlation has to cross a process boundary.
#[derive(Debug, Default)]
pub struct Counter {
    _private: (),
}

impl RequestIdSource for Counter {
    fn next_id(&self) -> http::HeaderValue {
        todo!()
    }
}

/// The header [`RequestId`] uses unless told otherwise.
///
/// A [`HeaderParams`] group rather than a name in a field, because the
/// description is built while the router is: `NAMES` is a `const`, and a name
/// chosen at run time is a name no document could have printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XRequestId(
    /// The identifier carried by this request.
    pub http::HeaderValue,
);

impl HeaderParams for XRequestId {
    const NAMES: &'static [&'static str] = &["x-request-id"];

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        vec![(
            http::HeaderName::from_static("x-request-id"),
            self.0.clone(),
        )]
    }
}

/// Assigns each request an identifier and echoes it back.
///
/// This is an interceptor because it adds a response header. Its contribution
/// keeps that wire-visible behavior in every covered operation's description,
/// and `H` is what makes the two the same fact: the header the description
/// names is the header the response carries, because there is only one place
/// the name is written.
pub struct RequestId<S = Counter, H = XRequestId> {
    source: S,
    trust_client: bool,
    _header: PhantomData<fn() -> H>,
}

// Hand-written for the reason `UncheckedInner`'s are: `PhantomData<fn() -> H>`
// needs nothing of `H`, and the source is what actually decides these.
impl<S: Clone, H> Clone for RequestId<S, H> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            trust_client: self.trust_client,
            _header: PhantomData,
        }
    }
}

impl<S: std::fmt::Debug, H: HeaderParams> std::fmt::Debug for RequestId<S, H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestId")
            .field("source", &self.source)
            .field("header", &H::NAMES)
            .field("trust_client", &self.trust_client)
            .finish_non_exhaustive()
    }
}

impl Default for RequestId<Counter, XRequestId> {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestId<Counter, XRequestId> {
    /// Uses `X-Request-Id`, generating one when the client sends none.
    #[must_use]
    pub fn new() -> Self {
        Self {
            source: Counter::default(),
            trust_client: false,
            _header: PhantomData,
        }
    }
}

impl<S: RequestIdSource, H: HeaderParams> RequestId<S, H> {
    /// Uses a different header group.
    ///
    /// Takes the group as a type parameter rather than a name, so that changing
    /// the header changes what every covered operation declares. A group
    /// naming more than one header sets and documents all of them.
    #[must_use]
    pub fn header<G: HeaderParams>(self) -> RequestId<S, G> {
        RequestId {
            source: self.source,
            trust_client: self.trust_client,
            _header: PhantomData,
        }
    }

    /// Echoes a client-supplied identifier instead of always generating one.
    ///
    /// Off by default. An inbound header is attacker-controlled, so letting it
    /// into logs and downstream requests is a decision worth making explicitly
    /// rather than a default worth inheriting.
    #[must_use]
    pub fn trust_client(mut self, trust: bool) -> Self {
        self.trust_client = trust;
        self
    }

    /// Replaces the identifier source.
    #[must_use]
    pub fn source<T: RequestIdSource>(self, source: T) -> RequestId<T, H> {
        RequestId {
            source,
            trust_client: self.trust_client,
            _header: PhantomData,
        }
    }
}

impl<C: Sync + 'static, S: RequestIdSource, H: HeaderParams + Send + Sync + 'static> Interceptor<C>
    for RequestId<S, H>
{
    type Reads = ();
    type Adds = H;

    /// Always continues: an identifier is added to whatever the chain returns.
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<H>, Infallible> {
        let _ = (
            request,
            reads,
            context,
            next,
            self.trust_client,
            &self.source,
        );
        todo!()
    }
}
