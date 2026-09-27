use std::time::Duration;

use super::{
    freshness::{self, CACHEABLE, HOP_BY_HOP, Unstorable},
    is_non_error, refuses_cross_origin,
};
use crate::http::{HeaderMap, HeaderValue, Method, StatusCode, header};

/// A header map from pairs.
fn map(fields: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(
            crate::http::HeaderName::from_bytes(name.as_bytes()).expect("a legal field name"),
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// A storable GET, for the cases that vary one thing.
fn storable(request: &[(&str, &str)], response: &[(&str, &str)]) -> Result<Duration, Unstorable> {
    freshness::storable(
        &Method::GET,
        StatusCode::OK,
        &map(request),
        &map(response),
        None,
    )
}

/// The baseline: a plain, explicitly cacheable response is stored.
///
/// The control for every refusal below. Without it, "these are refused" would
/// pass for an implementation that refused everything.
#[test]
fn an_explicitly_cacheable_response_is_stored() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=60")]),
        Ok(Duration::from_secs(60))
    );
}

/// One case per way a response is refused, counted against the variants.
#[test]
fn every_refusal_has_a_case() {
    let cases: &[(Unstorable, Result<Duration, Unstorable>)] = &[
        (
            Unstorable::Method,
            freshness::storable(
                &Method::POST,
                StatusCode::OK,
                &HeaderMap::new(),
                &map(&[("cache-control", "max-age=60")]),
                None,
            ),
        ),
        (
            Unstorable::Status,
            freshness::storable(
                &Method::GET,
                StatusCode::INTERNAL_SERVER_ERROR,
                &HeaderMap::new(),
                &map(&[("cache-control", "max-age=60")]),
                None,
            ),
        ),
        (
            Unstorable::RequestNoStore,
            storable(
                &[("cache-control", "no-store")],
                &[("cache-control", "max-age=60")],
            ),
        ),
        (
            Unstorable::ResponseNoStore,
            storable(&[], &[("cache-control", "max-age=60, no-store")]),
        ),
        (
            Unstorable::Private,
            storable(&[], &[("cache-control", "max-age=60, private")]),
        ),
        (
            Unstorable::NoCache,
            storable(&[], &[("cache-control", "max-age=60, no-cache")]),
        ),
        (
            Unstorable::VaryWildcard,
            storable(&[], &[("cache-control", "max-age=60"), ("vary", "*")]),
        ),
        (
            Unstorable::SetCookie,
            storable(
                &[],
                &[("cache-control", "max-age=60"), ("set-cookie", "a=1")],
            ),
        ),
        (
            Unstorable::Authorized,
            storable(
                &[("authorization", "Bearer x")],
                &[("cache-control", "max-age=60")],
            ),
        ),
        (Unstorable::NoFreshness, storable(&[], &[])),
    ];

    for (expected, actual) in cases {
        assert_eq!(actual, &Err(*expected), "{expected:?}");
    }

    // Counted against the enum, so a refusal added without a case fails the
    // build.
    let variants = [
        Unstorable::Method,
        Unstorable::Status,
        Unstorable::RequestNoStore,
        Unstorable::ResponseNoStore,
        Unstorable::Private,
        Unstorable::NoCache,
        Unstorable::VaryWildcard,
        Unstorable::SetCookie,
        Unstorable::Authorized,
        Unstorable::NoFreshness,
    ];

    // An exhaustive match, so an eleventh variant stops this compiling.
    for variant in variants {
        let _: &str = match variant {
            Unstorable::Method => "method",
            Unstorable::Status => "status",
            Unstorable::RequestNoStore => "request no-store",
            Unstorable::ResponseNoStore => "response no-store",
            Unstorable::Private => "private",
            Unstorable::NoCache => "no-cache",
            Unstorable::VaryWildcard => "vary: *",
            Unstorable::SetCookie => "set-cookie",
            Unstorable::Authorized => "authorized",
            Unstorable::NoFreshness => "no freshness",
        };
    }

    assert_eq!(cases.len(), variants.len(), "a refusal has no case");
}

/// A narrowed directive is read as the whole one.
///
/// `private="set-cookie"` narrows what must not be shared. Storing part of a
/// response is not something this cache can do, so the conservative reading is
/// the only correct one.
#[test]
fn a_narrowed_directive_is_read_as_the_whole_one() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=60, private=\"x\"")]),
        Err(Unstorable::Private)
    );
}

/// A credentialed request is stored only where the response says it may be.
#[test]
fn an_authenticated_response_is_stored_only_when_it_says_so() {
    for directive in ["max-age=60, public", "s-maxage=60"] {
        assert!(
            storable(
                &[("authorization", "Bearer x")],
                &[("cache-control", directive)]
            )
            .is_ok(),
            "{directive}"
        );
    }
}

/// `s-maxage` wins, because this is a shared cache and that is what it is for.
#[test]
fn the_shared_lifetime_wins_over_the_private_one() {
    assert_eq!(
        storable(&[], &[("cache-control", "max-age=10, s-maxage=99")]),
        Ok(Duration::from_secs(99))
    );
}

/// There is no heuristic freshness unless one was configured.
///
/// The single most important safety decision here: every heuristic is a guess
/// that turns a correct origin into an incorrect cache.
#[test]
fn a_response_that_said_nothing_is_not_reused_unless_a_default_was_set() {
    assert_eq!(storable(&[], &[]), Err(Unstorable::NoFreshness));

    assert_eq!(
        freshness::storable(
            &Method::GET,
            StatusCode::OK,
            &HeaderMap::new(),
            &HeaderMap::new(),
            Some(Duration::from_secs(30)),
        ),
        Ok(Duration::from_secs(30))
    );
}

/// Every cacheable status is one RFC 9110 lists, and 206 is not among them.
#[test]
fn the_cacheable_set_is_the_one_the_specification_names() {
    assert_eq!(
        CACHEABLE,
        [200, 203, 204, 300, 301, 308, 404, 405, 410, 414, 501]
    );

    // A 206 does arise -- `response::range` serves one -- and it stays out
    // anyway: this cache replays a stored response whole, so storing a part
    // would serve a partial body as a complete representation.
    assert!(!CACHEABLE.contains(&206));
}

/// The fields a stored response must not keep.
#[test]
fn a_stored_response_keeps_no_connection_specific_field() {
    let mut headers = map(&[
        ("connection", "keep-alive"),
        ("keep-alive", "timeout=5"),
        ("transfer-encoding", "chunked"),
        ("age", "42"),
        ("etag", "\"abc\""),
        ("content-type", "application/json"),
    ]);

    freshness::strip(&mut headers);

    for name in HOP_BY_HOP {
        assert!(!headers.contains_key(*name), "{name} survived");
    }
    // And the fields that are not connection-specific do survive.
    assert!(headers.contains_key(header::ETAG));
    assert!(headers.contains_key(header::CONTENT_TYPE));
}

/// `Vary` is read as a set: lowercased, sorted, deduplicated.
#[test]
fn the_vary_names_are_a_set() {
    assert_eq!(
        freshness::vary(&map(&[
            ("vary", "Accept-Encoding, origin"),
            ("vary", "ORIGIN")
        ])),
        ["accept-encoding", "origin"]
    );
}

/// A CORS response that does not vary on the origin is refused.
///
/// The mis-ordering case, caught without needing to know the order. Storing one
/// hands one origin's `Access-Control-Allow-Origin` to another, which defeats
/// the check entirely.
#[test]
fn a_cross_origin_response_that_does_not_vary_on_origin_is_refused() {
    assert!(refuses_cross_origin(&map(&[(
        "access-control-allow-origin",
        "https://app.example.com"
    )])));

    // The control: the same response, varying correctly.
    assert!(!refuses_cross_origin(&map(&[
        ("access-control-allow-origin", "https://app.example.com"),
        ("vary", "origin"),
    ])));

    // And a response with no CORS headers at all is not refused for this.
    assert!(!refuses_cross_origin(&map(&[("etag", "\"abc\"")])));
}

/// RFC 9111 section 4.4: "A non-error response is one with a 2xx (Successful)
/// or 3xx (Redirection) status code."
///
/// One case per class rather than per status, because the sentence is about
/// classes. The two that matter are the boundaries: a 3xx invalidates — a
/// `POST` answered with a redirect changed the resource just as much as one
/// answered 204 — and a 4xx does not, because a refused write changed nothing.
#[test]
fn every_status_class_is_classified_the_way_section_4_4_defines() {
    for status in [
        StatusCode::OK,
        StatusCode::CREATED,
        StatusCode::NO_CONTENT,
        StatusCode::MOVED_PERMANENTLY,
        StatusCode::SEE_OTHER,
        StatusCode::TEMPORARY_REDIRECT,
    ] {
        assert!(is_non_error(status), "{status} is 2xx or 3xx");
    }

    for status in [
        StatusCode::CONTINUE,
        StatusCode::BAD_REQUEST,
        StatusCode::NOT_FOUND,
        StatusCode::METHOD_NOT_ALLOWED,
        StatusCode::CONFLICT,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        assert!(!is_non_error(status), "{status} is neither 2xx nor 3xx");
    }
}

/// A response body that fails while `Cache` reads it.
///
/// In-crate because no public surface builds a body that fails on demand:
/// `Body::from_body` is `pub(crate)`.
mod failing {
    use std::{
        collections::HashMap,
        convert::Infallible,
        io,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll},
        time::Duration,
    };

    use bytes::Bytes;
    use http_body::{Frame, SizeHint};
    use http_body_util::BodyExt;

    use crate::{
        Router,
        extract::body::text::Text,
        http::{
            Request,
            body::{Body, BoxError},
        },
        middleware::{
            Continued, Interceptor, Next,
            cache::{
                Cache,
                store::{CacheStore, PrimaryKey, StoredResponse},
            },
        },
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    /// States 4096 octets, yields 1024 of them, then the connection fails.
    struct Failing(u8);

    impl http_body::Body for Failing {
        type Data = Bytes;
        type Error = BoxError;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
            self.0 += 1;
            Poll::Ready(match self.0 {
                1 => Some(Ok(Frame::data(Bytes::from(vec![b'a'; 1024])))),
                2 => Some(Err(Box::new(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "upstream went away",
                )))),
                _ => None,
            })
        }

        fn size_hint(&self) -> SizeHint {
            SizeHint::with_exact(4096)
        }
    }

    /// Replaces whatever the handler produced with a [`Failing`] body.
    struct FailPartWay;

    impl Interceptor<()> for FailPartWay {
        type Reads = ();
        type Adds = ();
        type Short = Infallible;

        async fn intercept(
            &self,
            request: Request,
            (): (),
            (): &(),
            next: Next<'_, ()>,
        ) -> Result<Continued, Infallible> {
            let mut continued = next.run(request).await;
            drop(continued.take_body());
            continued.set_body(Body::from_body(Failing(0)));
            Ok(continued)
        }
    }

    /// A store that keeps what it is given, so a refusal to store is visible.
    #[derive(Clone, Default)]
    struct Recorded(Arc<Mutex<HashMap<PrimaryKey, Vec<StoredResponse>>>>);

    impl CacheStore<()> for Recorded {
        async fn get(&self, key: &PrimaryKey, (): &()) -> Vec<StoredResponse> {
            self.0
                .lock()
                .expect("no test panics while holding this")
                .get(key)
                .cloned()
                .unwrap_or_default()
        }

        async fn put(&self, key: PrimaryKey, response: StoredResponse, (): &()) {
            self.0
                .lock()
                .expect("no test panics while holding this")
                .entry(key)
                .or_default()
                .push(response);
        }

        async fn invalidate(&self, key: &PrimaryKey, (): &()) {
            self.0
                .lock()
                .expect("no test panics while holding this")
                .remove(key);
        }
    }

    async fn page() -> Text {
        Text("x".repeat(4096))
    }

    /// The page with its body replaced by a failing one, beneath `cache` when
    /// one is given.
    fn service(cache: Option<Cache<Recorded>>) -> Service<()> {
        let endpoint = EndpointBuilder::new(
            Method::Get,
            PathTemplate::parse("/page").expect("a valid path"),
            page,
        );
        let router = Router::<()>::new().mount(endpoint);
        match cache {
            Some(cache) => router.intercept(cache).intercept(FailPartWay).build(()),
            None => router.intercept(FailPartWay).build(()),
        }
        .expect("a describable router")
    }

    /// Whether reading the response body to its end failed.
    async fn read_fails(service: &Service<()>) -> bool {
        let request = http::Request::builder()
            .method("GET")
            .uri("/page")
            .body(Body::empty())
            .expect("a well-formed request");
        service
            .call(request)
            .await
            .into_body()
            .collect()
            .await
            .is_err()
    }

    /// A read that fails part-way is handed on failing, as it is with no cache
    /// mounted, and is not stored: RFC 9111 section 3.3 forbids sending an
    /// incomplete response as a complete one.
    #[tokio::test]
    async fn a_body_that_fails_part_way_is_handed_on_failing_and_not_stored() {
        let store = Recorded::default();
        let cache = Cache::new(store.clone())
            .namespace("test")
            .default_freshness(Duration::from_secs(60));

        assert!(
            read_fails(&service(None)).await,
            "the control read succeeded"
        );
        assert!(
            read_fails(&service(Some(cache))).await,
            "a failed read reached the client as a complete body"
        );
        assert!(
            store
                .0
                .lock()
                .expect("no test panics while holding this")
                .is_empty(),
            "a body that failed part-way was stored"
        );
    }
}
