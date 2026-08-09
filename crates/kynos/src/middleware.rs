//! Middleware that declares what it does to the contract.
//!
//! # Why not `tower::Layer`
//!
//! A `Layer` can change the status, rewrite the body, add headers, or refuse
//! the request entirely, and the type system says nothing about which. Wrapping
//! an operation in one therefore invalidates its description in a way no tool
//! can detect. That is the single largest source of wrong OpenAPI documents in
//! the Rust ecosystem today.
//!
//! Kynos splits middleware in two:
//!
//! - An [`Interceptor`] can affect the exchange, and must declare an
//!   [`OperationContribution`] saying how. Contributions merge into every
//!   operation the interceptor covers.
//! - An [`Observer`] sees everything and changes nothing, so it needs to
//!   declare nothing. Logging, tracing and metrics live here.
//!
//! This is stricter than tower, and also *more* useful: you can write your own
//! interceptor, and attaching it to a group documents its effect on every
//! operation underneath automatically. With tower that mapping is maintained by
//! hand, and drifts.
//!
//! The `unchecked` feature restores `Layer` support for anyone who needs it,
//! at the price of a description marked non-authoritative.
//!
//! # Out-of-document middleware
//!
//! Some things are wire-visible but contract-neutral: no operation in `paths`
//! changes shape. CORS preflight, response compression, trailing-slash
//! normalization. These ship as ordinary configuration with no contribution,
//! because there is nothing to declare.

use std::future::Future;

use crate::http::{Request, Response};

/// What an interceptor adds to the description of every operation it covers.
///
/// A closed set, deliberately. If an interceptor does something not expressible
/// here, it is doing something OpenAPI cannot describe, and Kynos would rather
/// not have it.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct OperationContribution {
    /// Security requirements this interceptor enforces.
    pub security: Vec<kynos_openapi::SecurityRequirement>,

    /// Security schemes to register in `components`.
    pub security_schemes: Vec<(String, kynos_openapi::SecurityScheme)>,

    /// Parameters this interceptor reads.
    pub parameters: Vec<kynos_openapi::Parameter>,

    /// Responses this interceptor can produce on its own.
    pub responses: kynos_openapi::Responses,

    /// Headers this interceptor adds to responses.
    pub response_headers: Vec<(String, kynos_openapi::Header)>,

    /// Whether this interceptor marks covered operations deprecated.
    pub deprecated: bool,
}

impl OperationContribution {
    /// An empty contribution.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Declares a response this interceptor can produce.
    #[must_use]
    pub fn with_response(mut self, status: u16, response: kynos_openapi::Response) -> Self {
        let _ = &mut self;
        let _ = (status, response);
        todo!()
    }

    /// Declares a response header this interceptor adds.
    #[must_use]
    pub fn with_response_header(
        mut self,
        name: impl Into<String>,
        header: kynos_openapi::Header,
    ) -> Self {
        let _ = &mut self;
        let _ = (name, header);
        todo!()
    }

    /// Merges another contribution into this one.
    ///
    /// # Errors
    ///
    /// Returns [`ContributionConflict`] when both declare a different response
    /// for the same status, which is how two interceptors that disagree about
    /// what a 429 means are caught at build time rather than in production.
    pub fn merge(&mut self, other: Self) -> Result<(), ContributionConflict> {
        let _ = other;
        todo!()
    }
}

/// Two interceptors disagreed about the same part of the description.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("two interceptors declare different descriptions for `{field}`")]
pub struct ContributionConflict {
    /// What they disagreed about.
    pub field: String,
}

/// Middleware that can affect the exchange, and says how.
pub trait Interceptor<C>: Send + Sync + 'static {
    /// What this interceptor adds to every operation it covers.
    ///
    /// Called once per operation while the router is built, never per request.
    fn contribution(&self) -> OperationContribution;

    /// Handles a request, calling `next` to continue.
    fn intercept(
        &self,
        request: Request,
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Response> + Send;
}

/// The remainder of the interceptor chain.
#[derive(Debug)]
pub struct Next<'a, C> {
    _private: std::marker::PhantomData<&'a C>,
}

impl<C> Next<'_, C> {
    /// Runs the rest of the chain.
    pub async fn run(self, request: Request) -> Response {
        let _ = request;
        todo!()
    }
}

/// Middleware that observes without altering.
///
/// Because it cannot change the exchange, it contributes nothing to the
/// description — which is why an observer needs no declaration and can see
/// everything, including the headers no extractor will surface to a handler.
pub trait Observer<C>: Send + Sync + 'static {
    /// Called when a request arrives, before any interceptor.
    fn on_request(&self, request: &Request, context: &C);

    /// Called when a response is about to be written.
    fn on_response(&self, response: &Response, elapsed: std::time::Duration);

    /// Called when a handler panicked.
    fn on_panic(&self, payload: &(dyn std::any::Any + Send)) {
        let _ = payload;
    }
}

/// Request tracing: the one standard way to log at the operation level.
#[cfg(feature = "trace")]
pub mod trace {
    use crate::{http, middleware::Observer};

    /// Emits one `tracing` span per operation.
    ///
    /// The span is named by `operation_id` and carries `method`,
    /// `matched_path`, `operation_id`, `status`, `latency` and `request_id`.
    /// Handler bodies use plain `tracing::info!` and inherit it, so there is no
    /// per-endpoint logging middleware to attach and nothing to forget.
    ///
    /// `matched_path` is exactly the `paths` key from the description, which
    /// makes it the correct metric label: bounded cardinality, and it lines up
    /// with the documented operation.
    ///
    /// Choosing a subscriber remains the application's decision — Kynos depends
    /// on the `tracing` facade and nothing more.
    #[derive(Clone, Debug, Default)]
    pub struct Trace {
        _private: (),
    }

    impl Trace {
        /// Traces every operation at `INFO`.
        #[must_use]
        pub fn new() -> Self {
            todo!()
        }

        /// Sets the level spans are emitted at.
        #[must_use]
        pub fn level(self, level: tracing::Level) -> Self {
            let _ = level;
            todo!()
        }

        /// Records request headers matching these names on the span.
        ///
        /// Anything not listed is omitted, so a header carrying a credential
        /// cannot end up in a log by accident.
        #[must_use]
        pub fn record_headers(self, names: &'static [&'static str]) -> Self {
            let _ = names;
            todo!()
        }
    }

    impl<C> Observer<C> for Trace {
        fn on_request(&self, request: &http::Request, context: &C) {
            let _ = (request, context);
            todo!()
        }

        fn on_response(&self, response: &http::Response, elapsed: std::time::Duration) {
            let _ = (response, elapsed);
            todo!()
        }

        fn on_panic(&self, payload: &(dyn std::any::Any + Send)) {
            let _ = payload;
            todo!()
        }
    }
}

/// Correlation identifiers.
pub mod request_id {
    use crate::{
        http,
        middleware::{Interceptor, Next, OperationContribution},
    };

    /// Assigns each request an identifier and echoes it back.
    ///
    /// This is an interceptor because it adds a response header. Its
    /// contribution keeps that wire-visible behavior in every covered
    /// operation's description.
    #[derive(Clone, Debug, Default)]
    pub struct RequestId {
        _private: (),
    }

    impl RequestId {
        /// Uses `X-Request-Id`, generating one when the client sends none.
        #[must_use]
        pub fn new() -> Self {
            todo!()
        }

        /// Uses a different header name.
        #[must_use]
        pub fn header(self, name: &'static str) -> Self {
            let _ = name;
            todo!()
        }
    }

    impl<C: Sync + 'static> Interceptor<C> for RequestId {
        fn contribution(&self) -> OperationContribution {
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
}

/// Limits, and the responses they make possible.
pub mod limits {
    use super::{Interceptor, Next, OperationContribution};
    use crate::http;

    /// Caps the size of a request body.
    ///
    /// Contributes 413 to every covered operation — which is the point.
    /// Configuring a limit and documenting that the limit exists are the same
    /// action, so an API cannot quietly reject payloads it claims to accept.
    #[derive(Clone, Copy, Debug)]
    pub struct BodySize {
        /// The maximum body size, in bytes.
        pub limit: u64,
    }

    impl BodySize {
        /// Caps bodies at `bytes`.
        #[must_use]
        pub fn new(bytes: u64) -> Self {
            Self { limit: bytes }
        }

        /// This interceptor's contribution.
        #[must_use]
        pub fn contribution(&self) -> OperationContribution {
            todo!()
        }
    }

    impl<C: Sync + 'static> Interceptor<C> for BodySize {
        fn contribution(&self) -> OperationContribution {
            Self::contribution(self)
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

    /// Caps how long a handler may run.
    ///
    /// Contributes 504.
    #[derive(Clone, Copy, Debug)]
    pub struct Timeout {
        /// The maximum handler duration.
        pub limit: std::time::Duration,
    }

    impl Timeout {
        /// Limits handlers to `limit`.
        pub fn new(limit: std::time::Duration) -> Self {
            Self { limit }
        }

        /// This interceptor's contribution.
        pub fn contribution(&self) -> OperationContribution {
            todo!()
        }
    }

    impl<C: Sync + 'static> Interceptor<C> for Timeout {
        fn contribution(&self) -> OperationContribution {
            Self::contribution(self)
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

    /// Caps concurrent in-flight requests.
    ///
    /// Contributes 503 and a `Retry-After` response header.
    #[derive(Clone, Copy, Debug)]
    pub struct Concurrency {
        /// The maximum number of requests in flight at once.
        pub limit: usize,
    }

    impl Concurrency {
        /// Limits in-flight requests to `limit`.
        pub fn new(limit: usize) -> Self {
            Self { limit }
        }

        /// This interceptor's contribution.
        pub fn contribution(&self) -> OperationContribution {
            todo!()
        }
    }

    impl<C: Sync + 'static> Interceptor<C> for Concurrency {
        fn contribution(&self) -> OperationContribution {
            Self::contribution(self)
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
}

/// Rate limiting.
pub mod rate_limit {
    use std::future::Future;

    use super::{Interceptor, Next, OperationContribution};
    use crate::http;

    /// The result of consulting a rate-limit policy.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Decision {
        /// The request may continue.
        Allow {
            /// Requests remaining in the current window.
            remaining: u32,
        },
        /// The request must receive 429 without calling the handler.
        Deny {
            /// How long the client should wait before retrying.
            retry_after: std::time::Duration,
        },
    }

    /// Application policy used to identify clients and maintain counters.
    pub trait RateLimitPolicy<C>: Send + Sync + 'static {
        /// Decides whether this request may continue.
        fn check(
            &self,
            request: &http::Request,
            context: &C,
        ) -> impl Future<Output = Decision> + Send;
    }

    /// Limits request rate per client.
    ///
    /// Contributes 429, a `Retry-After` header, and the `RateLimit-*` headers.
    /// Kynos supplies the description and the response; the *policy* — how a
    /// client is identified, where counters live — is the application's, since
    /// prescribing a store would mean prescribing a dependency.
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use kynos::{
    ///     http,
    ///     middleware::rate_limit::{Decision, RateLimit, RateLimitPolicy},
    /// };
    ///
    /// #[derive(Clone, Debug)]
    /// struct PerClient;
    ///
    /// impl RateLimitPolicy<()> for PerClient {
    ///     async fn check(&self, _: &http::Request, _: &()) -> Decision {
    ///         Decision::Allow { remaining: 99 }
    ///     }
    /// }
    ///
    /// let limit = RateLimit::new(100, Duration::from_secs(60), PerClient);
    /// # let _ = limit;
    /// ```
    #[derive(Clone, Debug)]
    pub struct RateLimit<P> {
        policy: P,
        requests: u32,
        window: std::time::Duration,
    }

    impl<P> RateLimit<P> {
        /// Allows `requests` per `window`, consulting `policy` for each request.
        #[must_use]
        pub fn new(requests: u32, window: std::time::Duration, policy: P) -> Self {
            Self {
                policy,
                requests,
                window,
            }
        }

        /// This interceptor's contribution.
        #[must_use]
        pub fn contribution(&self) -> OperationContribution {
            todo!()
        }
    }

    impl<C: Sync + 'static, P: RateLimitPolicy<C>> Interceptor<C> for RateLimit<P> {
        fn contribution(&self) -> OperationContribution {
            Self::contribution(self)
        }

        async fn intercept(
            &self,
            request: http::Request,
            context: &C,
            next: Next<'_, C>,
        ) -> http::Response {
            let _ = (
                &self.policy,
                self.requests,
                self.window,
                request,
                context,
                next,
            );
            todo!()
        }
    }
}

/// Turning a panic into a documented response.
pub mod catch_panic {
    mod private {
        pub trait Sealed {}
    }

    /// A compile-time panic recovery policy.
    ///
    /// This trait is sealed. Select a policy through
    /// [`Router::catch_panics`](crate::router::Router::catch_panics),
    /// [`Group::catch_panics`](crate::router::group::Group::catch_panics), or
    /// [`EndpointBuilder::catch_panics`](crate::router::endpoint::EndpointBuilder::catch_panics)
    /// rather than naming its implementations in application code.
    pub trait PanicPolicy: private::Sealed + Send + Sync + 'static {}

    /// Lets a panic continue unwinding.
    ///
    /// This is the default policy and installs no recovery wrapper.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Propagate {
        _private: (),
    }

    impl private::Sealed for Propagate {}
    impl PanicPolicy for Propagate {}

    /// Converts a panic into a 500 problem document.
    ///
    /// Selecting this policy contributes a 500 response to every covered
    /// operation and requires the final binary to use `panic = "unwind"`.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Catch {
        _private: (),
    }

    impl private::Sealed for Catch {}
    impl PanicPolicy for Catch {}
}

/// Cross-origin resource sharing.
///
/// Out-of-document: a preflight `OPTIONS` is a browser protocol detail, not an
/// operation of the API, so it contributes nothing. Use
/// [`Cors::document_response_headers`](cors::Cors::document_response_headers) when the CORS response headers are part
/// of what you want clients to know about.
pub mod cors {
    use crate::{
        http,
        middleware::{Interceptor, Next, OperationContribution},
    };

    /// CORS configuration.
    #[derive(Clone, Debug, Default)]
    pub struct Cors {
        _private: (),
    }

    impl Cors {
        /// A configuration permitting nothing, to be widened deliberately.
        #[must_use]
        pub fn new() -> Self {
            todo!()
        }

        /// Permits these origins.
        #[must_use]
        pub fn allow_origins(self, origins: &'static [&'static str]) -> Self {
            let _ = origins;
            todo!()
        }

        /// Permits credentialed requests.
        #[must_use]
        pub fn allow_credentials(self) -> Self {
            todo!()
        }

        /// Also declares the CORS response headers in the description.
        #[must_use]
        pub fn document_response_headers(self) -> Self {
            todo!()
        }
    }

    impl<C: Sync + 'static> Interceptor<C> for Cors {
        fn contribution(&self) -> OperationContribution {
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
}

/// Response compression.
///
/// Out-of-document: content coding is transport, and OpenAPI does not model it.
#[cfg(feature = "compression")]
pub mod compression {
    use crate::{
        http,
        middleware::{Interceptor, Next, OperationContribution},
    };

    /// Compresses responses when the client accepts it.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Compression {
        _private: (),
    }

    impl Compression {
        /// Enables every compiled-in algorithm.
        #[must_use]
        pub fn new() -> Self {
            todo!()
        }

        /// Skips responses smaller than `bytes`.
        #[must_use]
        pub fn min_size(self, bytes: u64) -> Self {
            let _ = bytes;
            todo!()
        }
    }

    impl<C: Sync + 'static> Interceptor<C> for Compression {
        fn contribution(&self) -> OperationContribution {
            OperationContribution::none()
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
}
