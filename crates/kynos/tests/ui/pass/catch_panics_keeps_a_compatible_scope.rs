//! The passing half of `catch_panics_keeps_the_mounted_scopes`.
//!
//! The same shape -- group, `catch_panics`, intercept -- differing in exactly
//! the property under test: 413 and 408 are disjoint and neither interceptor
//! adds a response header, so carrying `S` across the policy change rejects
//! nothing.
//!
//! Without this control, the negative would pass just as well if `catch_panics`
//! stopped compiling after a `group` at all.

use kynos::{
    middleware::limits::{BodySize, Timeout},
    prelude::*,
};

fn main() {
    let _ = Router::<()>::new()
        .group(Group::<()>::new("/a").intercept(BodySize::new(1024)))
        .catch_panics()
        .intercept(Timeout::new(std::time::Duration::from_secs(30)));
}
