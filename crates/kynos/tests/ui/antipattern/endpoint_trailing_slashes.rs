//! README anti-pattern 10: nor can a single endpoint.

use kynos::{
    openapi, response::status::NoContent, router::endpoint::builder::EndpointBuilder,
    router::policy::TrailingSlashPolicy,
};

async fn health() -> NoContent {
    NoContent
}

fn main() {
    let _ = EndpointBuilder::<(), _, _>::new(
        openapi::Method::Get,
        openapi::PathTemplate::parse("/health").expect("valid path"),
        health,
    )
    .trailing_slashes(TrailingSlashPolicy::Redirect);
}
