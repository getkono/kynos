//! Correlation identifiers.

use crate::{
    http,
    middleware::{Interceptor, Next, contribution::OperationContribution},
    router::operation::Route,
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

/// Assigns each request an identifier and echoes it back.
///
/// This is an interceptor because it adds a response header. Its
/// contribution keeps that wire-visible behavior in every covered
/// operation's description.
pub struct RequestId<S = Counter> {
    source: S,
    _private: std::marker::PhantomData<fn() -> S>,
}

// Hand-written for the reason `UncheckedInner`'s are: `PhantomData<fn() -> S>`
// needs nothing of `S`, and the source is what actually decides these.
impl<S: Clone> Clone for RequestId<S> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            _private: std::marker::PhantomData,
        }
    }
}

impl<S: std::fmt::Debug> std::fmt::Debug for RequestId<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestId")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl Default for RequestId<Counter> {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestId<Counter> {
    /// Uses `X-Request-Id`, generating one when the client sends none.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }
}

impl<S: RequestIdSource> RequestId<S> {
    /// Uses a different header name.
    #[must_use]
    pub fn header(self, name: &'static str) -> Self {
        let _ = name;
        todo!()
    }

    /// Echoes a client-supplied identifier instead of always generating one.
    ///
    /// Off by default. An inbound header is attacker-controlled, so letting it
    /// into logs and downstream requests is a decision worth making explicitly
    /// rather than a default worth inheriting.
    #[must_use]
    pub fn trust_client(self, trust: bool) -> Self {
        let _ = trust;
        todo!()
    }

    /// Replaces the identifier source.
    #[must_use]
    pub fn source<T: RequestIdSource>(self, source: T) -> RequestId<T> {
        let _ = source;
        todo!()
    }
}

impl<C: Sync + 'static, S: RequestIdSource> Interceptor<C> for RequestId<S> {
    fn contribution(&self, _route: Route<'_>) -> OperationContribution {
        todo!()
    }

    async fn intercept(
        &self,
        request: http::Request,
        context: &C,
        next: Next<'_, C>,
    ) -> http::Response {
        let _ = (request, context, next);
        todo!()
    }
}
