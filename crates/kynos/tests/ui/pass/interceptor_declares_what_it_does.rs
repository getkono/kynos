//! The passing half of `interceptor_adds_what_it_declares`.
//!
//! An interceptor whose `Adds` is a group has to attach it to return at all.
//! This one does, so it compiles.

use std::convert::Infallible;

use kynos::{
    extract::params::header::HeaderParams,
    http::{HeaderName, HeaderValue, Request},
    middleware::{Continued, Interceptor, Next},
};

struct Stamp(HeaderValue);

impl HeaderParams for Stamp {
    const NAMES: &'static [&'static str] = &["x-stamp"];

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        vec![(HeaderName::from_static("x-stamp"), self.0.clone())]
    }
}

struct Stamping;

impl<C: Sync + 'static> Interceptor<C> for Stamping {
    type Reads = ();
    type Adds = Stamp;
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<Stamp>, Infallible> {
        let _ = (reads, context);
        Ok(next
            .run(request)
            .await
            .with_headers(Stamp(HeaderValue::from_static("1"))))
    }
}

fn intercepts<C: Sync + 'static, I: Interceptor<C>>() {}

fn main() {
    intercepts::<(), Stamping>();
}
