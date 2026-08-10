//! README anti-pattern 11: the emitted document is read-only.

fn patch(service: &mut kynos::router::service::Service<()>) {
    let _ = service.openapi_mut();
}

fn main() {}
