//! An interceptor that claims to cover fewer methods than it runs on does not
//! mount.
//!
//! The collision check reads which methods a stack element covers, and only a
//! `When` that `Group::intercept_when` builds covers fewer than every one. An
//! interceptor claiming a set of its own would be skipped by the check on
//! operations it still runs on, so mounting it fails to compile.

use std::convert::Infallible;

use kynos::{
    http::Request,
    middleware::{Continued, Interceptor, Next},
    openapi::Method,
    prelude::*,
    router::group::Methods,
};

struct Narrowed;

impl<C: Sync + 'static> Interceptor<C> for Narrowed {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    const __METHODS: Methods = Methods::only(&[Method::Get]);

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
    let _ = Router::<()>::new().intercept(Narrowed);
}
