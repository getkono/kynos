//! `EndpointBuilder::catch_panics` does not launder a colliding interceptor.
//!
//! The endpoint-scope twin of `catch_panics_keeps_the_interceptor_stack`. The
//! policy change keeps the interceptors added before it, so the second
//! `RequestId` is checked against the first. Returning
//! `EndpointBuilder<C, H, A, Catch>` -- which is
//! `EndpointBuilder<C, H, A, Catch, ()>` -- would check it against an empty
//! list while both wrote `x-request-id` to the same response.

use kynos::{
    middleware::request_id::RequestId, openapi, router::endpoint::builder::EndpointBuilder,
};

async fn health() {}

fn main() {
    let _ = EndpointBuilder::<(), _, _>::new(
        openapi::Method::Get,
        openapi::PathTemplate::parse("/health").expect("valid path"),
        health,
    )
    .intercept(RequestId::new())
    .catch_panics()
    .intercept(RequestId::new());
}
