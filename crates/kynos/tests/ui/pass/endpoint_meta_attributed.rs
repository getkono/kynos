use kynos::response::status::NoContent;

#[kynos::get("/health")]
async fn health() -> NoContent {
    NoContent
}

fn is_operation<T: kynos::router::endpoint::meta::EndpointMeta>() {}

fn main() {
    is_operation::<health>();
}
