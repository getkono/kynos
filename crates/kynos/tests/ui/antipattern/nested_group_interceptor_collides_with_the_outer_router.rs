//! A nested router's group-scoped interceptors reach the outer check.
//!
//! The worst of the family, because it is not an ordering accident: `nest`
//! compared only the nested router's *own* stack against the outer one, and a
//! nested router's type never carried what its groups held. So this pair was
//! accepted whichever order it was written in, and both `RequestId` covered
//! every operation under `/x/y` at run time.

use kynos::{middleware::request_id::RequestId, prelude::*};

fn main() {
    let _ = Router::<()>::new()
        .intercept(RequestId::new())
        .nest(
            "/x",
            Router::<()>::new().group(Group::<()>::new("/y").intercept(RequestId::new())),
        );
}
