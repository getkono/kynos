//! A `routes!` member is a handler and its `.intercept(..)` calls, so an
//! attribute on one is refused rather than left to remove an operation the
//! router would still describe.
//!
//! The control is `pass/routes_intercept_after_an_operation.rs`.

#[kynos::get("/users")]
async fn list() {}

// The interceptor is spelled out rather than imported: the macro refuses the
// member before it emits it, so an import would be unused and put a warning
// in the snapshot beside the diagnostic this case is about.
fn main() {
    let _ = kynos::Router::<()>::new().mount(kynos::routes![
        #[cfg(any())]
        list.intercept(kynos::middleware::limits::body_size::BodySize::new(1024))
    ]);
}
