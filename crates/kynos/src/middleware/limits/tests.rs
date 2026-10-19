//! `BodySize` over a request body that fails part-way.
//!
//! In-crate rather than in `tests/limits.rs` because no public surface builds a
//! body that fails: `Body::from_body` is `pub(crate)`, which is the reason
//! `extract/body/json_lines/tests.rs` lives in-crate too, and the reason the
//! failing body is [`middleware::testing`](crate::middleware::testing)'s. It is ungated, so this
//! module runs at baseline features; the one case that reads through a
//! streaming extractor carries that extractor's gate.

use super::body_size::BodySize;
use crate::{
    Router,
    http::StatusCode,
    middleware::testing::{interrupted, read, upload},
    openapi::{Method, PathTemplate},
    router::{endpoint::builder::EndpointBuilder, service::Service},
};

/// The upload operation, under `BodySize` when `limit` names one.
fn service(limit: Option<u64>) -> Service<()> {
    let endpoint = EndpointBuilder::new(
        Method::Post,
        PathTemplate::parse("/upload").expect("a valid path"),
        upload,
    );
    limited(Router::<()>::new().mount(endpoint), limit)
}

/// `router` built under `BodySize` when `limit` names one.
fn limited(router: Router<()>, limit: Option<u64>) -> Service<()> {
    match limit {
        Some(limit) => router.intercept(BodySize::new(limit)).build(()),
        None => router.build(()),
    }
    .expect("a describable router")
}

/// Mounting a limit must not turn a refused request into an accepted one: the
/// extractor beneath answers exactly as it does with no limit mounted. The
/// upload states no `Content-Length`, which is the branch that buffers.
#[tokio::test]
async fn a_body_that_fails_part_way_is_refused_as_it_is_without_the_limit() {
    let (unlimited, unlimited_body) = read(service(None).call(interrupted(None)).await).await;
    let (limited, limited_body) = read(service(Some(1024)).call(interrupted(None)).await).await;

    assert_eq!(unlimited, StatusCode::BAD_REQUEST);
    assert_eq!(
        limited,
        unlimited,
        "{}",
        String::from_utf8_lossy(&limited_body)
    );
    assert_eq!(limited_body, unlimited_body);
}

/// What a streaming extractor beneath the limit reads, which is where handing
/// on only the failure would differ from handing on what arrived with it.
#[cfg(all(feature = "json", feature = "openapi32"))]
mod streamed {
    use super::limited;
    use crate::{
        Router,
        http::StatusCode,
        middleware::testing::{interrupted_as, read, tally},
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// The tally operation, under `BodySize` when `limit` names one.
    fn service(limit: Option<u64>) -> Service<()> {
        let endpoint = EndpointBuilder::new(
            Method::Post,
            PathTemplate::parse("/upload").expect("a valid path"),
            tally,
        );
        limited(Router::<()>::new().mount(endpoint), limit)
    }

    /// The records that arrived before the failure reach the handler, and the
    /// failure after them: the read fails where it would with no limit
    /// mounted, not earlier.
    #[tokio::test]
    async fn records_before_a_failure_reach_a_streaming_handler_as_without_the_limit() {
        let request = || interrupted_as("application/x-ndjson", None, b"1\n2\n");
        let (unlimited, unlimited_body) = read(service(None).call(request()).await).await;
        let (limited, limited_body) = read(service(Some(1024)).call(request()).await).await;

        assert_eq!(unlimited, StatusCode::OK);
        assert_eq!(
            unlimited_body.as_ref(),
            b"read [1, 2], then a rejection",
            "{}",
            String::from_utf8_lossy(&unlimited_body)
        );
        assert_eq!(limited, unlimited);
        assert_eq!(
            limited_body,
            unlimited_body,
            "{}",
            String::from_utf8_lossy(&limited_body)
        );
    }
}
