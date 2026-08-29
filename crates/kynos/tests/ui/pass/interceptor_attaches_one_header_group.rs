//! The passing half of `interceptor_attaches_an_undeclared_header`.
//!
//! The same interceptor with the second attachment removed. It differs in
//! exactly the property under test -- how many groups reach one response --
//! so it fails if `with_headers` ever stops being callable at all, which is
//! the way a compile-fail case passes for the wrong reason.
//!
//! The empty group is attached deliberately rather than forwarded: `Adds` is
//! `()`, and `Continued<()>` is what `with_headers(())` produces, so the one
//! call this case makes is the one the negative makes twice.

use std::convert::Infallible;

use kynos::{
    http::Request,
    middleware::{Continued, Interceptor, Next},
};

struct Forwarding;

impl<C: Sync + 'static> Interceptor<C> for Forwarding {
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
        Ok(next.run(request).await.with_headers(()))
    }
}

fn main() {}
