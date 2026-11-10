//! The passing half of `interceptor_narrows_the_methods_it_covers`.
//!
//! The same interceptor, leaving the methods it covers at every one, mounts.

use std::convert::Infallible;

use kynos::{
    http::Request,
    middleware::{Continued, Interceptor, Next},
    prelude::*,
};

struct Covering;

impl<C: Sync + 'static> Interceptor<C> for Covering {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Infallible> {
        let _ = (reads, context);
        Ok(next.run(request).await)
    }
}

fn main() {
    let _ = Router::<()>::new().intercept(Covering);
}
