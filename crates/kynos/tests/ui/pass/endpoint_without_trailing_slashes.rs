use kynos::{openapi, response::status::NoContent, router::endpoint::builder::EndpointBuilder};

async fn health() -> NoContent {
    NoContent
}

fn main() {
    let _ = EndpointBuilder::<(), _, _>::new(
        openapi::Method::Get,
        openapi::PathTemplate::parse("/health").expect("valid path"),
        health,
    )
    .deprecated();
}
