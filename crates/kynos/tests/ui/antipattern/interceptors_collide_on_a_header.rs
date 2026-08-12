//! Two interceptors covering one route and adding the same response header do
//! not compile.
//!
//! `RequestId` adds `x-request-id`. So does this one, and the name is compared
//! case-insensitively, so `X-Request-Id` collides too.

use std::convert::Infallible;

use kynos::{
    extract::params::header::HeaderParams,
    http::{HeaderName, HeaderValue, Request},
    middleware::{Continued, Interceptor, Next, request_id::RequestId},
    prelude::*,
};

struct AlsoRequestId(HeaderValue);

impl HeaderParams for AlsoRequestId {
    const NAMES: &'static [&'static str] = &["X-Request-Id"];

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        vec![(HeaderName::from_static("x-request-id"), self.0.clone())]
    }
}

struct Stamping;

impl<C: Sync + 'static> Interceptor<C> for Stamping {
    type Reads = ();
    type Adds = AlsoRequestId;
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<AlsoRequestId>, Infallible> {
        let _ = (reads, context);
        Ok(next
            .run(request)
            .await
            .with_headers(AlsoRequestId(HeaderValue::from_static("1"))))
    }
}

fn main() {
    let _ = Router::<()>::new()
        .intercept(RequestId::new())
        .intercept(Stamping);
}
