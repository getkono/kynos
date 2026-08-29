//! `merge` carries the merged router's sub-stacks into the check too.
//!
//! The same defect `nest` had, at the same level rather than beneath a prefix.
//! A merged router's group-scoped interceptors were in no type the merging
//! router could see, so they were compared against nothing.

use kynos::{middleware::request_id::RequestId, prelude::*};

fn main() {
    let _ = Router::<()>::new()
        .intercept(RequestId::new())
        .merge(Router::<()>::new().group(Group::<()>::new("/y").intercept(RequestId::new())));
}
