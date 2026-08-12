//! Two interceptors covering one route and answering with the same status do
//! not compile.
//!
//! `BodySize` answers 413. So does this one, so a consumer meeting a 413 could
//! not tell which of them replied.

use kynos::{
    http::Request,
    middleware::{Continued, Interceptor, Next, limits::BodySize},
    prelude::*,
};

#[derive(Debug, thiserror::Error, kynos::ApiError)]
#[error("too big, again")]
#[problem(status = 413)]
struct AlsoTooLarge;

struct AlsoCaps;

impl<C: Sync + 'static> Interceptor<C> for AlsoCaps {
    type Reads = ();
    type Adds = ();
    type Short = AlsoTooLarge;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, AlsoTooLarge> {
        let _ = (reads, context);
        Ok(next.run(request).await)
    }
}

fn main() {
    let _ = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .intercept(AlsoCaps);
}
