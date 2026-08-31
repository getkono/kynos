//! The passing half of `catch_panics_keeps_the_interceptor_stack`.
//!
//! The same shape -- intercept, `catch_panics`, intercept -- differing in
//! exactly the property under test: 413 and 408 are disjoint and neither
//! interceptor adds a response header, so keeping the stack across the policy
//! change rejects nothing.
//!
//! Without this control, the negative would pass just as well if `catch_panics`
//! stopped compiling altogether.

use kynos::{
    middleware::limits::{BodySize, Timeout},
    prelude::*,
};

fn main() {
    let _ = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .catch_panics()
        .intercept(Timeout::new(std::time::Duration::from_secs(30)));
}
