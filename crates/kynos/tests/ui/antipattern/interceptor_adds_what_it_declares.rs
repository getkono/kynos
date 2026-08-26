//! Declaring a header group and never attaching it does not compile.
//!
//! `Adds` is not a claim checked against behaviour later; it is the return
//! type. An interceptor that names `Stamp` and forwards the chain's
//! `Continued<()>` has not produced what it said it would.

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

struct Forgetful;

impl<C: Sync + 'static> Interceptor<C> for Forgetful {
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
        Ok(next.run(request).await)
    }
}

fn main() {}
