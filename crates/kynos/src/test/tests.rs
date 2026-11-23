//! The request body a `TestRequest` sends: what it yields, and what it tells a
//! reader about its end and its length before the reader polls. And, in
//! [`assertions`], how each `TestResponse::assert_*` helper fails, and in
//! `connected`, what a client built `connect` reaches and refuses.
//!
//! `tests/client.rs` holds what a service reads from it; the end and length
//! signals are read by a body's consumer rather than by the frames, so they
//! are asserted here, on the private type.

use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

use bytes::Bytes;
use http_body::Body as HttpBody;

use super::body::{Framed, RequestBody};

fn framed(bytes: &'static str, remaining: Option<u64>) -> Framed {
    Framed {
        body: RequestBody::Whole(Bytes::from_static(bytes.as_bytes())),
        remaining,
        failed: false,
    }
}

/// The data of the next frame, `None` at the end, or the error's text.
fn next(body: &mut Framed) -> Option<Result<Bytes, String>> {
    let mut context = Context::from_waker(Waker::noop());
    match Pin::new(body).poll_frame(&mut context) {
        Poll::Ready(frame) => frame.map(|frame| {
            frame
                .map(|frame| frame.into_data().expect("only data frames"))
                .map_err(|error| error.to_string())
        }),
        Poll::Pending => panic!("a whole body never waits"),
    }
}

/// A whole body is one chunk, then the end — never an empty chunk after it.
#[test]
fn a_whole_body_ends_once_sent() {
    let mut body = RequestBody::Whole(Bytes::from_static(b"hi"));
    let mut context = Context::from_waker(Waker::noop());

    assert_eq!(
        body.poll_chunk(&mut context),
        Poll::Ready(Some(Bytes::from_static(b"hi")))
    );
    assert_eq!(body.poll_chunk(&mut context), Poll::Ready(None));
}

/// A whole body short of its declared length fails the read rather than
/// ending.
#[test]
fn a_whole_body_short_of_its_length_fails() {
    let mut body = framed("hi", Some(5));

    assert_eq!(next(&mut body), Some(Ok(Bytes::from_static(b"hi"))));
    let failure = next(&mut body).expect("a failure, not an end");
    assert!(
        failure
            .expect_err("a short body fails")
            .contains("3 octet(s) short"),
    );
    assert_eq!(next(&mut body), None);
    assert!(!body.is_end_stream(), "a short end is no end");
}

/// A declared length is the exact size until its last octet is read, which
/// ends the body.
#[test]
fn a_declared_length_is_the_size_and_its_end() {
    let mut body = framed("hello world", Some(5));
    assert!(!body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(5));

    assert_eq!(next(&mut body), Some(Ok(Bytes::from_static(b"hello"))));
    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
}

/// Without a declared length nothing is promised: no size, and no end until a
/// read finds one.
#[test]
fn an_undeclared_length_promises_nothing() {
    let mut body = framed("hello", None);
    let size = body.size_hint();
    assert_eq!((size.lower(), size.upper()), (0, None));
    assert!(!body.is_end_stream());

    assert_eq!(next(&mut body), Some(Ok(Bytes::from_static(b"hello"))));
    assert!(!body.is_end_stream());
    assert_eq!(next(&mut body), None);
}

/// `TestRequest::stalled_body`: a framed body owed one octet that never
/// arrives. `tests/client.rs` holds the 408 it reaches.
#[cfg(feature = "server")]
mod stalled_body {
    use std::{
        pin::Pin,
        task::{Context, Waker},
    };

    use http_body::Body as HttpBody;

    use crate::{
        Router,
        http::{HeaderValue, header},
        test::{
            TestClient,
            body::{Framed, RequestBody},
        },
    };

    /// The body never yields and never ends, and is framed by the one octet
    /// it owes, so a reader waits rather than finding an empty body.
    #[test]
    fn the_body_owes_one_octet_and_never_sends_it() {
        let mut body = Framed {
            body: RequestBody::Stalled,
            remaining: Some(1),
            failed: false,
        };
        let mut context = Context::from_waker(Waker::noop());

        assert!(Pin::new(&mut body).poll_frame(&mut context).is_pending());
        assert!(!body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(1));
    }

    /// `stalled_after`'s body yields its first octets, then never yields nor
    /// ends again, and declares no length of its own.
    #[test]
    fn a_body_stalled_after_its_first_octets_sends_them_then_waits() {
        let mut body = Framed {
            body: RequestBody::StalledAfter(bytes::Bytes::from_static(b"hel")),
            remaining: None,
            failed: false,
        };
        let mut context = Context::from_waker(Waker::noop());

        let first = Pin::new(&mut body).poll_frame(&mut context);
        assert!(matches!(
            first,
            std::task::Poll::Ready(Some(Ok(ref frame))) if frame.data_ref().is_some_and(|data| data == "hel")
        ));
        assert!(Pin::new(&mut body).poll_frame(&mut context).is_pending());
        assert!(!body.is_end_stream());
        assert_eq!(body.size_hint().exact(), None);
    }

    /// The request declares the octet it owes and the media type, and its
    /// body is the stalled one.
    #[test]
    fn the_request_declares_its_length_and_media_type() {
        let client = TestClient::new(
            Router::<()>::new()
                .build(())
                .expect("an empty router is describable"),
        );
        let request = client.post("/").stalled_body("application/json");

        assert!(request.body.is_stalled());
        assert_eq!(request.content_length, Some(1));
        assert_eq!(
            request.headers.get(header::CONTENT_LENGTH),
            Some(&HeaderValue::from_static("1"))
        );
        assert_eq!(
            request.headers.get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
    }
}

/// What a client sets once seeds every request it begins, and what a request
/// sets itself replaces it, read off the request `send` would assemble.
mod defaults {
    use std::net::SocketAddr;

    use crate::{
        Router,
        http::{HeaderValue, header},
        test::TestClient,
    };

    fn client() -> TestClient<()> {
        TestClient::new(
            Router::<()>::new()
                .build(())
                .expect("an empty router is describable"),
        )
    }

    fn address(port: u16) -> SocketAddr {
        SocketAddr::from(([192, 0, 2, 10], port))
    }

    /// Every request a client begins starts from its peer and its fields, not
    /// only the first.
    #[test]
    fn every_request_starts_from_the_client_defaults() {
        let client = client()
            .with_peer(address(40000))
            .with_default_header("x-tenant", "acme");

        for request in [client.get("/a"), client.post("/b")] {
            assert_eq!(request.peer, Some(address(40000)));
            assert_eq!(
                request.headers.get("x-tenant"),
                Some(&HeaderValue::from_static("acme"))
            );
        }
    }

    /// A client that sets nothing leaves its requests as empty as before.
    #[test]
    fn a_client_without_defaults_begins_empty_requests() {
        let client = client();
        let request = client.get("/a");

        assert_eq!(request.peer, None);
        assert!(request.headers.is_empty());
    }

    /// A request's own peer and field win, and only for the name it sets: the
    /// default under another name still goes.
    #[test]
    fn a_request_overrides_the_default_it_names() {
        let client = client()
            .with_peer(address(40000))
            .with_default_header("x-tenant", "acme")
            .with_default_header("x-trace", "on");

        let request = client
            .get("/a")
            .peer(address(50000))
            .header("X-Tenant", "globex");

        assert_eq!(request.peer, Some(address(50000)));
        assert_eq!(
            request
                .headers
                .get_all("x-tenant")
                .iter()
                .collect::<Vec<_>>(),
            [&HeaderValue::from_static("globex")]
        );
        assert_eq!(
            request.headers.get("x-trace"),
            Some(&HeaderValue::from_static("on"))
        );
    }

    /// A second default under one name replaces the first, as a second
    /// `header` does, rather than sending both.
    #[test]
    fn a_second_default_for_a_name_replaces_the_first() {
        let client = client()
            .with_default_header("x-tenant", "acme")
            .with_default_header("x-tenant", "globex")
            .with_peer(address(1))
            .with_peer(address(2));
        let request = client.get("/a");

        assert_eq!(
            request
                .headers
                .get_all("x-tenant")
                .iter()
                .collect::<Vec<_>>(),
            [&HeaderValue::from_static("globex")]
        );
        assert_eq!(request.peer, Some(address(2)));
    }

    /// RFC 6750 section 2.1: `Bearer`, one space, then the token, replacing a
    /// default credential.
    #[test]
    fn bearer_presents_the_token_in_authorization() {
        let client = client().with_default_header("authorization", "Bearer default");
        let request = client.get("/a").bearer("mF_9.B5f-4.1JqM");

        assert_eq!(
            request
                .headers
                .get_all(header::AUTHORIZATION)
                .iter()
                .collect::<Vec<_>>(),
            [&HeaderValue::from_static("Bearer mF_9.B5f-4.1JqM")]
        );
    }

    /// A token outside `b64token` is still sent, so a test can present one a
    /// service must refuse.
    #[test]
    fn bearer_sends_a_token_outside_the_grammar_as_written() {
        let client = client();
        let request = client.get("/a").bearer("not a b64token");

        assert_eq!(
            request.headers.get(header::AUTHORIZATION),
            Some(&HeaderValue::from_static("Bearer not a b64token"))
        );
    }

    #[test]
    #[should_panic(expected = "`a b` is not a header name")]
    fn a_default_header_name_http_cannot_carry_panics() {
        let _ = client().with_default_header("a b", "c");
    }

    #[test]
    #[should_panic(expected = "`a\nb` is not a header value")]
    fn a_default_header_value_http_cannot_carry_panics() {
        let _ = client().with_default_header("x-a", "a\nb");
    }

    #[test]
    #[should_panic(expected = "`Bearer a\nb` is not a header value")]
    fn a_bearer_token_no_field_can_carry_panics() {
        let client = client();
        let _ = client.get("/a").bearer("a\nb");
    }

    /// The connection both in-process transports insert: from loopback where
    /// nothing names a peer, to loopback, and a connection rather than the
    /// no-socket sentinel, as `connect`'s real one is.
    #[test]
    fn a_request_naming_no_peer_arrives_on_a_loopback_connection() {
        let loopback = SocketAddr::from(([127, 0, 0, 1], 0));

        let defaulted = crate::test::connection_from(None);
        assert_eq!(defaulted.peer_addr(), Some(loopback));
        assert_eq!(defaulted.local_addr(), Some(loopback));
        assert!(!defaulted.is_in_process());

        let named = crate::test::connection_from(Some(address(40000)));
        assert_eq!(named.peer_addr(), Some(address(40000)));
        assert_eq!(named.local_addr(), Some(loopback));
        assert!(!named.is_in_process());
    }

    /// The base reaches every response the client receives, and the last
    /// one set is the one joined.
    #[tokio::test]
    async fn a_problem_base_reaches_every_response() {
        let service = crate::router::service::Service::<()>::new(
            kynos_openapi::Document::new(
                kynos_openapi::SpecVersion::V3_1,
                kynos_openapi::Info::new("Test", "1"),
            ),
            |_| async {
                let mut response = crate::http::Response::new(crate::http::body::Body::from_bytes(
                    bytes::Bytes::from_static(br#"{"type":"https://errors.example.com/gone"}"#),
                ));
                *response.status_mut() = crate::http::StatusCode::GONE;
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/problem+json"),
                );
                response
            },
        );
        let client = TestClient::new(service)
            .with_problem_base("https://elsewhere.example/")
            .with_problem_base("https://errors.example.com/");

        for _ in 0..2 {
            client.get("/a").send().await.assert_problem(410, "gone");
        }
        client
            .get("/a")
            .send_streaming()
            .await
            .finish()
            .await
            .assert_problem(410, "gone");
    }

    #[test]
    #[should_panic(
        expected = "`errors.example.com/` is not a problem base: one begins with a URI \
                               scheme, as `https://errors.example.com/` does"
    )]
    fn a_problem_base_with_no_scheme_panics() {
        let _ = client().with_problem_base("errors.example.com/");
    }

    /// A request's own cookies join the client's rather than replacing them,
    /// and one it names itself is sent instead of the client's, once.
    #[test]
    fn default_cookies_join_a_requests_own_which_win_by_name() {
        let client = client()
            .with_default_cookie("session", "abc123")
            .with_default_cookie("theme", "light");
        let request = client
            .get("/a")
            .cookie("theme", "dark")
            .cookie("lang", "en");

        assert_eq!(
            request.cookie_field().as_deref(),
            Some("session=abc123; theme=dark; lang=en")
        );
        assert_eq!(
            client.get("/b").cookie_field().as_deref(),
            Some("session=abc123; theme=light")
        );
    }

    /// A second default under one name replaces the first where it stood.
    #[test]
    fn a_second_default_cookie_for_a_name_replaces_the_first() {
        let client = client()
            .with_default_cookie("a", "1")
            .with_default_cookie("b", "2")
            .with_default_cookie("a", "3");

        assert_eq!(client.get("/a").cookie_field().as_deref(), Some("a=3; b=2"));
    }

    /// A `Cookie` field the request writes itself is sent as written, and a
    /// request with no cookies anywhere sends none.
    #[test]
    fn a_requests_own_cookie_field_wins_over_default_cookies() {
        let client = client().with_default_cookie("session", "abc123");
        let request = client.get("/a").header("cookie", "raw=1");

        assert_eq!(request.cookie_field(), None);
        assert_eq!(
            request.headers.get(header::COOKIE),
            Some(&HeaderValue::from_static("raw=1"))
        );
        assert_eq!(self::client().get("/a").cookie_field(), None);
    }

    /// Either default would silently replace the other, so the second one
    /// given is refused, in both orders.
    #[test]
    #[should_panic(
        expected = "`with_default_cookie` would replace the client's `Cookie` \
                               default header"
    )]
    fn a_default_cookie_beside_a_default_cookie_header_panics() {
        let _ = client()
            .with_default_header("Cookie", "session=abc123")
            .with_default_cookie("theme", "dark");
    }

    #[test]
    #[should_panic(expected = "a `Cookie` default header would replace the client's \
                               `with_default_cookie`s")]
    fn a_default_cookie_header_beside_a_default_cookie_panics() {
        let _ = client()
            .with_default_cookie("theme", "dark")
            .with_default_header("cookie", "session=abc123");
    }

    #[test]
    #[should_panic(expected = "`a=b\nc` is not a header value")]
    fn a_default_cookie_no_field_can_carry_panics() {
        let _ = client().with_default_cookie("a", "b\nc");
    }
}

/// An in-process response carries the `Content-Length` a connection frames it
/// with, RFC 9110 section 8.6's exceptions included.
mod content_length {
    use bytes::Bytes;

    use super::framed;
    use crate::{
        Router,
        http::{HeaderValue, Method, Response, StatusCode, body::Body, header},
        test::{TestClient, with_content_length},
    };

    fn answered(method: &Method, status: StatusCode, body: Body) -> Option<HeaderValue> {
        let mut response = Response::new(body);
        *response.status_mut() = status;
        with_content_length(method, response)
            .headers()
            .get(header::CONTENT_LENGTH)
            .cloned()
    }

    fn whole(bytes: &'static str) -> Body {
        Body::from_bytes(Bytes::from_static(bytes.as_bytes()))
    }

    #[test]
    fn a_body_of_known_length_states_it() {
        assert_eq!(
            answered(&Method::GET, StatusCode::OK, whole("hello")),
            Some(HeaderValue::from_static("5"))
        );
    }

    /// An empty body is a length of zero, as a connection frames it, not an
    /// absent field.
    #[test]
    fn an_empty_body_states_zero() {
        assert_eq!(
            answered(&Method::POST, StatusCode::ACCEPTED, Body::empty()),
            Some(HeaderValue::from_static("0"))
        );
    }

    /// A streamed body's length is unknown until its end, so a connection
    /// chunks it and states none.
    #[test]
    fn a_body_of_unknown_length_states_none() {
        assert_eq!(
            answered(
                &Method::GET,
                StatusCode::OK,
                Body::from_body(framed("hello", None))
            ),
            None
        );
    }

    /// Each response RFC 9110 section 8.6 either forbids the field on or
    /// gives the length of another response, against a control that states
    /// it.
    #[test]
    fn the_responses_section_8_6_excepts_state_none() {
        for (method, status) in [
            (Method::HEAD, StatusCode::OK),
            (Method::GET, StatusCode::CONTINUE),
            (Method::GET, StatusCode::NO_CONTENT),
            (Method::GET, StatusCode::NOT_MODIFIED),
            (Method::CONNECT, StatusCode::OK),
        ] {
            assert_eq!(
                answered(&method, status, whole("hello")),
                None,
                "{method} -> {status}"
            );
        }
        assert_eq!(
            answered(&Method::CONNECT, StatusCode::BAD_REQUEST, whole("hello")),
            Some(HeaderValue::from_static("5"))
        );
    }

    /// A length the service stated stands, and a transfer coding is never
    /// joined by one.
    #[test]
    fn a_framing_field_already_there_is_kept() {
        for (name, value) in [
            (header::CONTENT_LENGTH, "9"),
            (header::TRANSFER_ENCODING, "chunked"),
        ] {
            let mut response = Response::new(whole("hello"));
            response
                .headers_mut()
                .insert(name.clone(), HeaderValue::from_static(value));
            let response = with_content_length(&Method::GET, response);

            assert_eq!(
                response.headers().get_all(&name).iter().collect::<Vec<_>>(),
                [&HeaderValue::from_static(value)]
            );
            if name == header::TRANSFER_ENCODING {
                assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
            }
        }
    }

    /// `send` over a service carries the field, the same value a served
    /// response arrives with.
    #[tokio::test]
    async fn an_in_process_response_states_what_a_served_one_does() {
        let service = || {
            Router::<()>::new()
                .build(())
                .expect("an empty router is describable")
        };
        let direct = TestClient::new(service()).get("/a").send().await;
        let length = direct
            .header("content-length")
            .expect("an in-process response states its length");
        assert_eq!(length, direct.bytes().len().to_string());

        #[cfg(feature = "server")]
        {
            let served = TestClient::through_server(crate::server::Server::new(service()))
                .get("/a")
                .send()
                .await;
            assert_eq!(served.header("content-length"), Some(length));
        }
    }
}

/// Each `assert_*` helper fails, once per way its `# Panics` section names,
/// with the message a test author reads.
///
/// A helper that never fails passes every suite written against it, so each
/// failure mode is held by its message rather than by a panic of any kind.
mod assertions {
    use bytes::Bytes;

    use crate::{
        http::{HeaderName, HeaderValue, Method, Response, StatusCode, body::Body},
        test::{IntoStatus, TestResponse},
    };

    /// A received response, as `TestRequest::send` leaves one, answering
    /// `GET /things`.
    fn received(status: StatusCode, headers: &[(&str, &str)], body: &'static [u8]) -> TestResponse {
        let body = Bytes::from_static(body);
        let mut response = Response::new(Body::from_bytes(body.clone()));
        *response.status_mut() = status;
        for (name, value) in headers {
            response.headers_mut().append(
                HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        TestResponse {
            method: Method::GET,
            path: "/things".to_owned(),
            response,
            body,
            problem_base: None,
        }
    }

    /// `received`, from a client built with `with_problem_base(base)`.
    fn based(base: &str, status: StatusCode, body: &'static [u8]) -> TestResponse {
        TestResponse {
            problem_base: Some(base.into()),
            ..received(status, &[PROBLEM], body)
        }
    }

    /// A slug is joined to the base verbatim, as `problem_type!` joins one,
    /// and a type with a scheme is compared as given.
    #[test]
    fn a_slug_is_joined_to_the_problem_base_and_a_uri_is_not() {
        let response = based(
            "https://errors.example.com/",
            StatusCode::FORBIDDEN,
            br#"{"type":"https://errors.example.com/insufficient-scope","status":403}"#,
        );
        response
            .assert_problem(403, "insufficient-scope")
            .assert_problem_type("insufficient-scope")
            .assert_problem(403, "https://errors.example.com/insufficient-scope");

        based("https://errors.example.com/", StatusCode::NOT_FOUND, b"{}")
            .assert_problem(404, "about:blank")
            .assert_problem_type("about:blank");
    }

    /// The failure names the type the slug was joined into, which is what the
    /// body was compared against.
    #[test]
    #[should_panic(expected = "GET /things -> 403: expected problem type \
                    `https://errors.example.com/insufficient-scope`, received `urn:example:other`")]
    fn assert_problem_names_the_joined_type_it_expected() {
        based(
            "https://errors.example.com/",
            StatusCode::FORBIDDEN,
            br#"{"type":"urn:example:other"}"#,
        )
        .assert_problem(403, "insufficient-scope");
    }

    #[test]
    #[should_panic(expected = "GET /things -> 403: expected problem type \
                    `https://errors.example.com/insufficient-scope`, received `about:blank`")]
    fn assert_problem_type_names_the_joined_type_it_expected() {
        based("https://errors.example.com/", StatusCode::FORBIDDEN, b"{}")
            .assert_problem_type("insufficient-scope");
    }

    /// Without a base a slug is compared as written, as every type was.
    #[test]
    #[should_panic(expected = "expected problem type `insufficient-scope`")]
    fn a_slug_without_a_base_is_compared_as_written() {
        received(
            StatusCode::FORBIDDEN,
            &[PROBLEM],
            br#"{"type":"https://errors.example.com/insufficient-scope"}"#,
        )
        .assert_problem(403, "insufficient-scope");
    }

    /// RFC 3986 section 3.1's scheme, and section 4.2's relative references,
    /// which never open with one.
    #[test]
    fn a_scheme_is_what_rfc_3986_says_opens_an_absolute_uri() {
        for absolute in [
            "about:blank",
            "urn:example:x",
            "https://e.example/",
            "a+b-c.d:x",
        ] {
            assert!(crate::test::has_scheme(absolute), "{absolute}");
        }
        for relative in ["slug", "a/b:c", "1x:y", ":x", "", "./a:b", "-a:b"] {
            assert!(!crate::test::has_scheme(relative), "{relative}");
        }
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 404: expected 200 OK, received 404 Not Found\n\
                               body: gone"
    )]
    fn assert_status_fails_on_another_status_and_shows_the_body() {
        received(StatusCode::NOT_FOUND, &[], b"gone").assert_status(StatusCode::OK);
    }

    /// The reason follows the exchange, so the message still opens with the
    /// request, and the body still follows the statuses.
    #[test]
    #[should_panic(
        expected = "GET /things -> 200: a revoked token is dead: expected 401 Unauthorized, \
                    received 200 OK\nbody: welcome"
    )]
    fn assert_status_because_fails_naming_the_reason_and_shows_the_body() {
        received(StatusCode::OK, &[], b"welcome")
            .assert_status_because(401, "a revoked token is dead");
    }

    #[test]
    fn assert_status_because_passes_on_the_status_and_chains() {
        received(StatusCode::UNAUTHORIZED, &[("x-id", "7")], b"")
            .assert_status_because(StatusCode::UNAUTHORIZED, "unused")
            .assert_header("x-id", "7");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 200: rows are numbered: `1000` is not a status code, which is 100 to 999"
    )]
    fn assert_status_because_names_the_reason_for_a_number_no_status_has() {
        received(StatusCode::OK, &[], b"").assert_status_because(1000, "rows are numbered");
    }

    /// The response comes back whole, so it stays readable after the
    /// assertion.
    #[test]
    fn expect_status_hands_back_the_response_it_asserted() {
        let response = received(StatusCode::CREATED, &[("x-id", "7")], b"made").expect_status(201);

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.header("x-id"), Some("7"));
        assert_eq!(response.text(), "made");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 404: expected 200 OK, received 404 Not Found\n\
                               body: gone"
    )]
    fn expect_status_fails_as_assert_status_does() {
        let _ = received(StatusCode::NOT_FOUND, &[], b"gone").expect_status(StatusCode::OK);
    }

    /// Every field, repeated ones included, with no name given.
    #[test]
    fn header_map_holds_every_field_received() {
        let response = received(
            StatusCode::OK,
            &[("set-cookie", "a=1"), ("set-cookie", "b=2"), ("x-id", "7")],
            b"",
        );
        let map = response.header_map();

        assert_eq!(map.len(), 3);
        assert_eq!(
            map.get_all("set-cookie").iter().collect::<Vec<_>>(),
            [
                &HeaderValue::from_static("a=1"),
                &HeaderValue::from_static("b=2")
            ]
        );
        assert_eq!(map.get("x-id"), Some(&HeaderValue::from_static("7")));
    }

    /// An unsuffixed literal is a `u16`, read as the status it numbers.
    #[test]
    fn assert_status_takes_a_bare_number() {
        received(StatusCode::NOT_FOUND, &[], b"").assert_status(404);
    }

    #[test]
    #[should_panic(expected = "GET /things -> 404: expected 200 OK, received 404 Not Found")]
    fn assert_status_fails_on_another_number() {
        received(StatusCode::NOT_FOUND, &[], b"").assert_status(200);
    }

    /// `StatusCode::from_u16`'s range, 100 to 999, bounds what a number may
    /// name; one outside it is a mistaken table, not a status that differs.
    #[test]
    #[should_panic(
        expected = "GET /things -> 200: `1000` is not a status code, which is 100 to 999"
    )]
    fn assert_status_refuses_a_number_no_status_has() {
        received(StatusCode::OK, &[], b"").assert_status(1000);
    }

    #[test]
    #[should_panic(expected = "GET /things -> 200: `99` is not a status code, which is 100 to 999")]
    fn assert_status_refuses_a_number_below_every_status() {
        received(StatusCode::OK, &[], b"").assert_status(99);
    }

    /// The two ends of the range are statuses.
    #[test]
    fn into_status_reads_both_ends_of_the_range() {
        assert_eq!(100.into_status(), StatusCode::CONTINUE);
        assert_eq!(999.into_status().as_u16(), 999);
        assert_eq!(StatusCode::OK.into_status(), StatusCode::OK);
    }

    #[test]
    #[should_panic(expected = "`1000` is not a status code, which is 100 to 999")]
    fn into_status_refuses_a_number_no_status_has() {
        let _ = 1000.into_status();
    }

    #[test]
    #[should_panic(expected = "GET /things -> 400: the body is not a problem document")]
    fn assert_problem_type_fails_on_a_body_that_is_not_json() {
        received(StatusCode::BAD_REQUEST, &[], b"plain").assert_problem_type("about:blank");
    }

    #[test]
    #[should_panic(expected = "GET /things -> 400: the problem document's `type` is not a string")]
    fn assert_problem_type_fails_on_a_type_that_is_not_a_string() {
        received(StatusCode::BAD_REQUEST, &[], br#"{"type":7}"#).assert_problem_type("about:blank");
    }

    /// An omitted `type` is `about:blank` (RFC 9457 section 3.1.1), and is
    /// reported as that rather than as an absence.
    #[test]
    #[should_panic(
        expected = "GET /things -> 400: expected problem type `urn:example:x`, received `about:blank`"
    )]
    fn assert_problem_type_fails_on_another_type_reading_an_omitted_one_as_blank() {
        received(StatusCode::BAD_REQUEST, &[], br#"{"status":400}"#)
            .assert_problem_type("urn:example:x");
    }

    /// A problem response, as `Problem` renders one.
    const PROBLEM: (&str, &str) = ("content-type", "application/problem+json");

    /// The media type is compared by its essence: parameters aside, and
    /// without regard to case (RFC 9110 section 8.3.1).
    #[test]
    fn assert_problem_passes_on_a_whole_problem() {
        received(
            StatusCode::CONFLICT,
            &[("content-type", "Application/Problem+JSON; charset=utf-8")],
            br#"{"type":"urn:example:taken","status":409}"#,
        )
        .assert_problem(409, "urn:example:taken")
        .assert_problem(StatusCode::CONFLICT, "urn:example:taken");
    }

    /// A document omitting both `type` and `status` is `about:blank`, with no
    /// member to disagree with the status line.
    #[test]
    fn assert_problem_reads_an_omitted_type_as_blank_and_an_omitted_status_as_agreeing() {
        received(StatusCode::NOT_FOUND, &[PROBLEM], b"{}").assert_problem(404, "about:blank");
    }

    #[test]
    #[should_panic(expected = "GET /things -> 404: expected 409 Conflict, received 404 Not Found")]
    fn assert_problem_fails_on_another_status() {
        received(StatusCode::NOT_FOUND, &[PROBLEM], b"{}").assert_problem(409, "about:blank");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 404: expected `Content-Type: application/problem+json`, \
                    and the response carries none"
    )]
    fn assert_problem_fails_on_an_absent_media_type() {
        received(StatusCode::NOT_FOUND, &[], b"{}").assert_problem(404, "about:blank");
    }

    /// A problem document sent as plain JSON is not one to a client that
    /// dispatches on the media type (RFC 9457 section 6.1).
    #[test]
    #[should_panic(
        expected = "GET /things -> 404: expected `Content-Type: application/problem+json`, \
                    received `Content-Type: application/json`"
    )]
    fn assert_problem_fails_on_another_media_type() {
        received(
            StatusCode::NOT_FOUND,
            &[("content-type", "application/json")],
            b"{}",
        )
        .assert_problem(404, "about:blank");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 404: expected problem type `urn:example:x`, received `about:blank`"
    )]
    fn assert_problem_fails_on_another_type() {
        received(StatusCode::NOT_FOUND, &[PROBLEM], b"{}").assert_problem(404, "urn:example:x");
    }

    /// RFC 9457 section 3.1.2 binds a generator to the same status in the
    /// member and on the status line.
    #[test]
    #[should_panic(
        expected = "GET /things -> 404: the problem document's `status` is 410, and the status line 404"
    )]
    fn assert_problem_fails_on_a_status_member_the_status_line_disagrees_with() {
        received(StatusCode::NOT_FOUND, &[PROBLEM], br#"{"status":410}"#)
            .assert_problem(404, "about:blank");
    }

    /// Every registered member is read into its field, and only the rest are
    /// extensions.
    #[test]
    fn problem_reads_the_registered_members_and_keeps_the_rest_as_extensions() {
        let problem = received(
            StatusCode::BAD_REQUEST,
            &[PROBLEM],
            br#"{"type":"urn:example:x","title":"Bad","status":400,"detail":"no",
                 "instance":"/things/1","errors":[{"pointer":"/a","detail":"short"}]}"#,
        )
        .problem();

        assert_eq!(problem.type_uri, "urn:example:x");
        assert_eq!(problem.title.as_deref(), Some("Bad"));
        assert_eq!(problem.status, Some(400));
        assert_eq!(problem.detail.as_deref(), Some("no"));
        assert_eq!(problem.instance.as_deref(), Some("/things/1"));
        assert_eq!(
            problem.extension::<Vec<crate::error::problem::PointerError>>("errors"),
            Some(vec![crate::error::problem::PointerError::new(
                "/a", "short"
            )])
        );
        assert_eq!(problem.extension::<String>("retry"), None);
    }

    #[test]
    fn problem_reads_an_omitted_type_as_blank() {
        let problem = received(StatusCode::NOT_FOUND, &[], b"{}").problem();
        assert_eq!(problem.type_uri, "about:blank");
        assert_eq!(problem.status, None);
    }

    #[test]
    #[should_panic(expected = "GET /things -> 400: the body is not a problem document: ")]
    fn problem_fails_on_a_body_that_is_not_json() {
        let _ = received(StatusCode::BAD_REQUEST, &[], b"plain").problem();
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 400: the body is not a problem document, which is a JSON object"
    )]
    fn problem_fails_on_json_that_is_not_an_object() {
        let _ = received(StatusCode::BAD_REQUEST, &[], b"[]").problem();
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 400: the problem document's `detail` is not a string"
    )]
    fn problem_fails_on_a_registered_member_of_another_type() {
        let _ = received(StatusCode::BAD_REQUEST, &[], br#"{"detail":7}"#).problem();
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 400: the problem document's `status` is not a status code"
    )]
    fn problem_fails_on_a_status_given_as_text() {
        let _ = received(StatusCode::BAD_REQUEST, &[], br#"{"status":"400"}"#).problem();
    }

    /// `StatusCode`'s range bounds the member as it bounds an expected status.
    #[test]
    #[should_panic(
        expected = "GET /things -> 400: the problem document's `status` is not a status code"
    )]
    fn problem_fails_on_a_status_no_status_code_has() {
        let _ = received(StatusCode::BAD_REQUEST, &[], br#"{"status":1000}"#).problem();
    }

    #[test]
    #[should_panic(expected = "the problem document's `retry` is not a `u64`")]
    fn extension_fails_on_a_member_of_another_type() {
        let _ = received(StatusCode::BAD_REQUEST, &[], br#"{"retry":"soon"}"#)
            .problem()
            .extension::<u64>("retry");
    }

    /// A registered member is never among the extensions, so reading one here
    /// would report it absent whatever the document carried.
    #[test]
    #[should_panic(expected = "`detail` is a member RFC 9457 registers, not an extension")]
    fn extension_refuses_a_registered_member() {
        let _ = received(StatusCode::BAD_REQUEST, &[], br#"{"detail":"no"}"#)
            .problem()
            .extension::<String>("detail");
    }

    #[test]
    #[should_panic(expected = "GET /things -> 200: `x-trace` is not on the response")]
    fn assert_header_fails_on_an_absent_field() {
        received(StatusCode::OK, &[], b"").assert_header("x-trace", "1");
    }

    #[test]
    #[should_panic(expected = "GET /things -> 200: expected `x-trace: 1`, received `x-trace: 2`")]
    fn assert_header_fails_on_another_value() {
        received(StatusCode::OK, &[("x-trace", "2")], b"").assert_header("x-trace", "1");
    }

    /// A field name is matched without regard to case (RFC 9110 section 5.1).
    #[test]
    fn assert_no_header_passes_on_an_absent_field() {
        received(StatusCode::OK, &[("x-other", "1")], b"").assert_no_header("WWW-Authenticate");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 401: expected no `WWW-Authenticate`, received \
                    `WWW-Authenticate: Basic`, `WWW-Authenticate: Bearer`"
    )]
    fn assert_no_header_fails_on_a_present_field_and_names_every_value() {
        received(
            StatusCode::UNAUTHORIZED,
            &[
                ("www-authenticate", "Basic"),
                ("www-authenticate", "Bearer"),
            ],
            b"",
        )
        .assert_no_header("WWW-Authenticate");
    }

    /// A name no field can have would pass against any response, so it is
    /// refused rather than passed.
    #[test]
    #[should_panic(expected = "GET /things -> 200: `x trace` is not a header name")]
    fn assert_no_header_refuses_a_name_no_field_can_have() {
        received(StatusCode::OK, &[], b"").assert_no_header("x trace");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 200: no `Set-Cookie` names `session`; the response set [\"theme\"]"
    )]
    fn assert_cookie_fails_on_a_cookie_never_set_and_names_the_ones_that_were() {
        received(StatusCode::OK, &[("set-cookie", "theme=dark; Path=/")], b"")
            .assert_cookie("session", "s");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 200: expected cookie `theme=light`, received `theme=dark`"
    )]
    fn assert_cookie_fails_on_another_value() {
        received(StatusCode::OK, &[("set-cookie", "theme=dark; Path=/")], b"")
            .assert_cookie("theme", "light");
    }

    #[test]
    #[should_panic(expected = "GET /things -> 200: expected a redirect, received 200 OK")]
    fn assert_redirect_fails_on_a_status_that_is_not_a_redirect() {
        received(StatusCode::OK, &[("location", "/next")], b"").assert_redirect("/next");
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 303: expected `location: /next`, received `location: /elsewhere`"
    )]
    fn assert_redirect_fails_on_another_location() {
        received(StatusCode::SEE_OTHER, &[("location", "/elsewhere")], b"")
            .assert_redirect("/next");
    }

    #[test]
    #[should_panic(expected = "GET /things -> 200: expected 206, received 200 OK")]
    fn assert_part_fails_on_a_status_other_than_206() {
        received(StatusCode::OK, &[("content-range", "bytes 0-1/4")], b"ab").assert_part(0, 1, 4);
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 206: expected `content-range: bytes 0-1/4`, \
                               received `content-range: bytes 0-2/4`"
    )]
    fn assert_part_fails_on_another_span() {
        received(
            StatusCode::PARTIAL_CONTENT,
            &[("content-range", "bytes 0-2/4")],
            b"abc",
        )
        .assert_part(0, 1, 4);
    }

    #[test]
    #[should_panic(
        expected = "GET /things -> 206: `Content-Range` names 2 octet(s) and the body carries 3"
    )]
    fn assert_part_fails_on_a_body_the_field_does_not_describe() {
        received(
            StatusCode::PARTIAL_CONTENT,
            &[("content-range", "bytes 0-1/4")],
            b"abc",
        )
        .assert_part(0, 1, 4);
    }

    /// A span ending before it starts names no octets (RFC 9110 section 14.4
    /// calls such a field invalid), and is refused as that rather than
    /// overflowing the length subtraction, which panics in a debug build and
    /// wraps in a release one, even against a response whose field repeats it.
    #[test]
    #[should_panic(expected = "GET /things -> 206: `bytes 5-2` names no octets: \
                               its last position precedes its first")]
    fn assert_part_refuses_a_span_that_ends_before_it_starts() {
        received(
            StatusCode::PARTIAL_CONTENT,
            &[("content-range", "bytes 5-2/10")],
            b"",
        )
        .assert_part(5, 2, 10);
    }

    /// The widest span `u64` can write is one octet more than `u64` counts,
    /// and is compared rather than overflowed.
    #[test]
    #[should_panic(
        expected = "GET /things -> 206: `Content-Range` names 18446744073709551616 octet(s) \
                    and the body carries 0"
    )]
    fn assert_part_compares_the_widest_span_without_overflowing() {
        received(
            StatusCode::PARTIAL_CONTENT,
            &[(
                "content-range",
                "bytes 0-18446744073709551615/18446744073709551615",
            )],
            b"",
        )
        .assert_part(0, u64::MAX, u64::MAX);
    }

    /// The readers that panic name the request as the assertions do.
    #[test]
    #[should_panic(expected = "GET /things -> 200: the body is not text")]
    fn text_fails_on_a_body_that_is_not_utf8_and_names_the_request() {
        let _ = received(StatusCode::OK, &[], b"\xff").text();
    }

    #[cfg(feature = "json")]
    #[test]
    #[should_panic(expected = "GET /things -> 200: the body is not valid JSON for `u8`")]
    fn json_fails_on_a_body_that_is_not_the_type_and_names_the_request() {
        let _: u8 = received(StatusCode::OK, &[], b"\"x\"").json();
    }

    /// How a `Set-Cookie` field reads, and how the cookie assertions tell a
    /// live cookie from a removal.
    #[cfg(feature = "cookie")]
    mod set_cookie {
        use std::time::{Duration, SystemTime};

        use super::received;
        use crate::{
            http::StatusCode,
            response::cookie::{Cookie, SameSite},
            test::{TestResponse, cookie::TestSetCookie},
        };

        fn setting(fields: &[&str]) -> TestResponse {
            let headers: Vec<(&str, &str)> =
                fields.iter().map(|field| ("set-cookie", *field)).collect();
            received(StatusCode::OK, &headers, b"")
        }

        fn encoded(cookie: &Cookie) -> String {
            cookie
                .encode()
                .expect("a representable cookie")
                .to_str()
                .expect("an ASCII field")
                .to_owned()
        }

        /// Every attribute the writer can spell reads back as it was set.
        #[test]
        fn every_attribute_the_writer_spells_reads_back() {
            let field = encoded(
                &Cookie::new("session", "abc123")
                    .path("/app")
                    .domain(".example.com")
                    .max_age(Duration::from_secs(3600))
                    .http_only()
                    .same_site(SameSite::Strict)
                    .partitioned(),
            );
            let response = setting(&[&field]);

            assert_eq!(
                response.set_cookie("session"),
                Some(TestSetCookie {
                    name: "session",
                    value: "abc123",
                    path: Some("/app"),
                    domain: Some("example.com"),
                    max_age: Some(Duration::from_secs(3600)),
                    expires: None,
                    secure: true,
                    http_only: true,
                    same_site: Some(SameSite::Strict),
                    partitioned: true,
                })
            );
        }

        /// RFC 6265 section 5.2: attribute names in any case, the last of a
        /// repeated one winning, and one a user agent ignores read as absent.
        #[test]
        fn attributes_read_as_a_user_agent_reads_them() {
            let response = setting(&[
                " a = 1 ;path=/x; PATH=/y; max-age=10; MAX-AGE=1e3; samesite=lax; \
                 SameSite=bogus; domain=; HTTPONLY; Expires=Sun, 06 Nov 1994 08:49:37 GMT",
                "b=2; Path=relative; Max-Age=99999999999999999999999",
            ]);

            let a = response.set_cookie("a").expect("`a` is set");
            assert_eq!((a.name, a.value), ("a", "1"));
            assert_eq!(a.path, Some("/y"));
            assert_eq!(a.max_age, Some(Duration::from_secs(10)));
            assert_eq!(a.same_site, None);
            assert_eq!(a.domain, None);
            assert!(a.http_only && !a.secure && !a.partitioned);
            assert_eq!(
                a.expires,
                Some(SystemTime::UNIX_EPOCH + Duration::from_secs(784_111_777))
            );

            let b = response.set_cookie("b").expect("`b` is set");
            assert_eq!(b.path, None);
            assert_eq!(b.max_age, Some(Duration::from_secs(u64::MAX)));
        }

        /// A removal is a `Max-Age` of zero or less, or a past `Expires`
        /// where no `Max-Age` overrides it; an empty value alone is not one.
        #[test]
        fn a_removal_is_told_from_an_empty_cookie() {
            let removal = encoded(&Cookie::removal("sid"));
            let response = setting(&[
                &removal,
                "negative=x; Max-Age=-1",
                "expired=x; Expires=Sun, 06 Nov 1994 08:49:37 GMT",
                "kept=x; Max-Age=60; Expires=Sun, 06 Nov 1994 08:49:37 GMT",
                "later=x; Expires=Fri, 31 Dec 9999 23:59:59 GMT",
                "empty=",
            ]);
            let removed = |name| {
                response
                    .set_cookie(name)
                    .unwrap_or_else(|| panic!("`{name}` is set"))
                    .is_removal()
            };

            assert_eq!(response.set_cookie("sid").map(|sid| sid.value), Some(""));
            assert!(removed("sid"));
            assert!(removed("negative"));
            assert!(removed("expired"));
            assert!(!removed("kept"));
            assert!(!removed("later"));
            assert!(!removed("empty"));
        }

        /// `Expires` reads as RFC 6265 section 5.1.1's cookie-date, so the
        /// dashed date a removal is commonly written with reads as one, and
        /// a date that algorithm refuses reads as absent.
        #[test]
        fn expires_reads_as_a_cookie_date() {
            let response = setting(&[
                "dashed=; Expires=Thu, 01-Jan-1970 00:00:00 GMT",
                "loose=x; Expires=1 JAN 2000 1:2:3",
                "two-digit=x; Expires=Wed, 09 Jun 21 10:18:14 GMT",
                "earliest=x; Expires=Mon, 01 Jan 1601 00:00:00 GMT",
                "too-early=x; Expires=Sun, 31 Dec 1600 23:59:59 GMT",
                "leap-second=x; Expires=Thu, 01 Jan 1970 00:00:60 GMT",
                "no-such-day=x; Expires=Tue, 31 Feb 2026 00:00:00 GMT",
                "no-time=x; Expires=Thu, 01 Jan 1970",
            ]);
            let expires = |name| {
                response
                    .set_cookie(name)
                    .unwrap_or_else(|| panic!("`{name}` is set"))
                    .expires
            };
            let at = |seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);

            assert_eq!(expires("dashed"), Some(SystemTime::UNIX_EPOCH));
            response.assert_cookie_cleared("dashed");
            assert_eq!(expires("loose"), Some(at(946_684_800 + 3723)));
            assert_eq!(expires("two-digit"), Some(at(1_623_233_894)));
            assert_eq!(
                expires("earliest"),
                SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(11_644_473_600))
            );
            assert_eq!(expires("too-early"), None);
            assert_eq!(expires("leap-second"), None);
            assert_eq!(expires("no-such-day"), None);
            assert_eq!(expires("no-time"), None);
        }

        /// Of several fields naming one cookie, the last is the one a user
        /// agent is left holding; a field naming no cookie is skipped.
        #[test]
        fn the_last_field_naming_a_cookie_is_the_one_read() {
            let response = setting(&["sid=old", "sid=; Max-Age=0", "sid=new", "no-pair", "=x"]);

            assert_eq!(response.set_cookie("sid").map(|sid| sid.value), Some("new"));
            assert_eq!(response.set_cookie("no-pair"), None);
            assert_eq!(response.set_cookie(""), None);
            assert_eq!(response.set_cookie("SID"), None);
        }

        #[test]
        fn assert_cookie_set_passes_on_a_live_cookie_and_chains() {
            setting(&["sid=; Max-Age=0", "sid=r4nd0m; HttpOnly", "theme="])
                .assert_cookie_set("sid")
                .assert_cookie_set("theme");
        }

        #[test]
        #[should_panic(
            expected = "GET /things -> 200: expected cookie `sid` set, the response cleared it: \
                        `Set-Cookie: sid=; Max-Age=0`"
        )]
        fn assert_cookie_set_fails_on_a_removal() {
            setting(&["sid=; Max-Age=0"]).assert_cookie_set("sid");
        }

        #[test]
        #[should_panic(
            expected = "GET /things -> 200: no `Set-Cookie` names `sid`; the response set [\"theme\"]"
        )]
        fn assert_cookie_set_fails_on_a_cookie_never_set_and_names_the_ones_that_were() {
            setting(&["theme=dark", "not a cookie"]).assert_cookie_set("sid");
        }

        #[test]
        fn assert_cookie_cleared_passes_on_a_removal_and_chains() {
            setting(&["sid=; Max-Age=0", "theme=; Max-Age=0"])
                .assert_cookie_cleared("sid")
                .assert_cookie_cleared("theme");
        }

        #[test]
        #[should_panic(
            expected = "GET /things -> 200: expected cookie `sid` cleared, the response set it: \
                        `Set-Cookie: sid=; Path=/`"
        )]
        fn assert_cookie_cleared_fails_on_an_empty_live_cookie() {
            setting(&["sid=; Path=/"]).assert_cookie_cleared("sid");
        }

        #[test]
        #[should_panic(
            expected = "GET /things -> 200: no `Set-Cookie` names `sid`; the response set []"
        )]
        fn assert_cookie_cleared_fails_on_a_cookie_never_set() {
            setting(&[]).assert_cookie_cleared("sid");
        }
    }
}

/// What `TestClient::method` reads a method from: a name as the method it
/// spells, case and all, and a typed one as itself.
mod methods {
    use crate::{http::Method, test::IntoMethod};

    #[test]
    fn a_name_is_the_method_it_spells() {
        assert_eq!("POST".into_method(), Method::POST);
        assert_eq!("QUERY".into_method().as_str(), "QUERY");
        // An extension method no registry names is sent as written.
        assert_eq!("PURGE".into_method().as_str(), "PURGE");
    }

    #[test]
    fn a_typed_method_is_itself() {
        assert_eq!(Method::PATCH.into_method(), Method::PATCH);
        assert_eq!((&Method::DELETE).into_method(), Method::DELETE);
    }

    #[test]
    #[should_panic(expected = "`GE T` is not a method, which is a token (RFC 9110 section 9.1)")]
    fn a_name_that_is_not_a_token_panics() {
        let _ = "GE T".into_method();
    }

    #[test]
    #[should_panic(expected = "`` is not a method")]
    fn an_empty_name_panics() {
        let _ = "".into_method();
    }

    /// Section 9.1 makes `post` another method than `POST`, which a test
    /// spelling it so almost never means to send.
    #[test]
    #[should_panic(
        expected = "`post` is not `POST`: a method is case-sensitive (RFC 9110 section 9.1); \
                    write `POST`, or pass `Method::from_bytes(b\"post\")` to send the \
                    extension method `post`"
    )]
    fn a_registered_method_in_another_case_panics() {
        let _ = "post".into_method();
    }

    /// The typed method stays the way to send that extension method.
    #[test]
    fn a_typed_method_in_another_case_is_sent_as_written() {
        let method = Method::from_bytes(b"post").expect("a token");
        assert_eq!(method.into_method().as_str(), "post");
    }
}

/// Witnesses a type that crosses a task boundary, and is shared across one.
fn assert_send_sync<T: Send + Sync>() {}

/// A client is shared, since `TestRequest::send` borrows it, so one behind an
/// `Arc` drives requests from several spawned tasks — whether or not it was
/// built over a server, since an enum's auto traits are every variant's.
#[test]
fn a_client_is_send_and_sync() {
    assert_send_sync::<super::TestClient<()>>();
}

/// Witnesses a future a task can be spawned with.
fn assert_send<T: Send>(_: &T) {}

/// What a shared client is for: a request sent from a spawned task, through
/// the `&TestClient` an `Arc` hands it. Compiled, never run.
const _: fn(&super::TestClient<()>) = |client| assert_send(&client.get("/").send());

/// A stream is read from a spawned task as a response is, through the same
/// shared client. Compiled, never run.
const _: fn(&super::TestClient<()>) = |client| assert_send(&client.get("/").send_streaming());

/// How a `TestStream` reads a body that arrives in frames: what each read
/// returns, and when the body is filed for `assert_conformance`.
mod streaming {
    use std::{collections::VecDeque, sync::Mutex};

    use bytes::{Bytes, BytesMut};
    use http_body::Frame;

    use crate::{
        http::{
            HeaderMap, HeaderValue, Method, Response, StatusCode, Uri,
            body::{Body, BoxError},
        },
        test::{Observed, TestEvent, TestStream},
    };

    /// Frames handed out one per poll, in order.
    struct Frames(VecDeque<Frame<Bytes>>);

    impl http_body::Body for Frames {
        type Data = Bytes;
        type Error = BoxError;

        fn poll_frame(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            std::task::Poll::Ready(self.get_mut().0.pop_front().map(Ok))
        }
    }

    /// A client's record holding one exchange whose body has not ended.
    fn record() -> Mutex<Vec<Observed>> {
        Mutex::new(vec![Observed {
            method: Method::GET,
            target: "/feed".to_owned(),
            uri: Uri::from_static("/feed"),
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: None,
        }])
    }

    /// A 200 answering `GET /feed` with `body`, filed in `observed`.
    fn stream(body: Body, observed: &Mutex<Vec<Observed>>) -> TestStream<'_> {
        let (parts, body) = Response::new(body).into_parts();
        TestStream {
            method: Method::GET,
            path: "/feed".to_owned(),
            parts,
            body: Some(body),
            received: BytesMut::new(),
            read: 0,
            observed,
            record: 0,
            problem_base: Some("https://errors.example.com/".into()),
        }
    }

    /// `text` cut into frames at `cuts`, which ascend.
    fn framed(text: &'static str, cuts: &[usize]) -> Body {
        let mut frames = VecDeque::new();
        let mut start = 0;
        for &cut in cuts.iter().chain([&text.len()]) {
            frames.push_back(Frame::data(Bytes::from_static(
                &text.as_bytes()[start..cut],
            )));
            start = cut;
        }
        Body::from_body(Frames(frames))
    }

    /// A feed with a keep-alive between events, a multi-line value, every
    /// field, and a last record the body ends before terminating.
    const FEED: &str = "event: tick\nid: 1\ndata: a\n\n: ping\n\n\
                        data: b\ndata: c\nretry: 50\n\n\ndata: d";

    /// The events `FEED` carries, written out rather than parsed.
    fn feed_events() -> Vec<TestEvent> {
        vec![
            TestEvent {
                data: "a".to_owned(),
                event: Some("tick".to_owned()),
                id: Some("1".to_owned()),
                retry: None,
            },
            TestEvent {
                data: "b\nc".to_owned(),
                retry: Some(50),
                ..TestEvent::default()
            },
            TestEvent {
                data: "d".to_owned(),
                ..TestEvent::default()
            },
        ]
    }

    /// Where the transport splits a body changes no event: every placement
    /// of two cuts, delimiters and multi-line values split included, reads
    /// as the events the whole feed carries, and then as the end.
    #[tokio::test]
    async fn events_read_the_same_wherever_the_frames_split() {
        for first in 0..=FEED.len() {
            for second in first..=FEED.len() {
                let observed = record();
                let mut stream = stream(framed(FEED, &[first, second]), &observed);
                let mut events = Vec::new();
                while let Some(event) = stream.next_event().await {
                    events.push(event);
                }
                assert_eq!(events, feed_events(), "cut at {first} and {second}");
                assert_eq!(stream.next_event().await, None);
            }
        }
    }

    /// One position serves both reads: a chunk after an event is what the
    /// event's frame held beyond it, and the next frame after that.
    #[tokio::test]
    async fn a_chunk_after_an_event_is_the_rest_of_its_frame() {
        let observed = record();
        let mut stream = stream(framed("data: a\n\ndata: b", &[11]), &observed);

        assert_eq!(
            stream.next_event().await.map(|event| event.data),
            Some("a".to_owned())
        );
        assert_eq!(stream.next_chunk().await, Some(Bytes::from_static(b"da")));
        assert_eq!(
            stream.next_chunk().await,
            Some(Bytes::from_static(b"ta: b"))
        );
        assert_eq!(stream.next_chunk().await, None);
        assert_eq!(stream.next_event().await, None);
    }

    /// A frame carrying no octets is not a chunk: an empty data frame and a
    /// trailers frame are passed over, and the end follows the last octets.
    #[tokio::test]
    async fn a_chunk_is_never_an_empty_or_trailers_frame() {
        let mut trailers = HeaderMap::new();
        trailers.insert("checksum", HeaderValue::from_static("abc"));
        let frames = Frames(
            [
                Frame::data(Bytes::from_static(b"a")),
                Frame::data(Bytes::new()),
                Frame::data(Bytes::from_static(b"b")),
                Frame::trailers(trailers),
            ]
            .into(),
        );
        let observed = record();
        let mut stream = stream(Body::from_body(frames), &observed);

        assert_eq!(stream.next_chunk().await, Some(Bytes::from_static(b"a")));
        assert_eq!(stream.next_chunk().await, Some(Bytes::from_static(b"b")));
        assert_eq!(stream.next_chunk().await, None);
    }

    /// An event record that is not UTF-8 fails the read, naming the exchange.
    #[tokio::test]
    #[should_panic(expected = "GET /feed -> 200: an event record is not text")]
    async fn an_event_record_that_is_not_text_fails_the_read() {
        let observed = record();
        let mut stream = stream(
            Body::from_bytes(Bytes::from_static(b"data: \xff\n\n")),
            &observed,
        );

        let _ = stream.next_event().await;
    }

    /// The body is filed once it ends, whole, and not before: until then
    /// conformance has only the head to hold.
    #[tokio::test]
    async fn the_body_is_filed_once_it_ends() {
        let observed = record();
        let mut stream = stream(framed("data: a\n\ndata: b\n\n", &[9]), &observed);

        assert!(stream.next_event().await.is_some());
        assert_eq!(observed.lock().unwrap()[0].body, None);
        assert!(stream.next_event().await.is_some());
        assert_eq!(stream.next_event().await, None);
        assert_eq!(
            observed.lock().unwrap()[0].body,
            Some(Bytes::from_static(b"data: a\n\ndata: b\n\n"))
        );
    }

    /// `finish` returns every octet, the ones already read included, and
    /// files them.
    #[tokio::test]
    async fn finish_returns_the_whole_body() {
        let observed = record();
        let mut stream = stream(framed(FEED, &[3, 30]), &observed);

        assert!(stream.next_event().await.is_some());
        let response = stream.finish().await;

        assert_eq!(response.text(), FEED);
        assert_eq!(response.events(), feed_events());
        assert_eq!(
            observed.lock().unwrap()[0].body,
            Some(Bytes::from_static(FEED.as_bytes()))
        );
    }

    /// The finished response keeps the client's problem base, as a sent one
    /// does.
    #[tokio::test]
    async fn finish_keeps_the_problem_base() {
        let observed = record();
        stream(
            framed(r#"{"type":"https://errors.example.com/gone"}"#, &[]),
            &observed,
        )
        .finish()
        .await
        .assert_problem_type("gone");
    }

    /// A body failing part-way fails the read, naming the exchange.
    #[tokio::test]
    #[should_panic(
        expected = "GET /feed -> 200: the response body failed part-way: connection reset"
    )]
    async fn a_body_failing_part_way_fails_the_read() {
        let observed = record();
        let mut stream = stream(
            Body::failed_after(Bytes::from_static(b"data: a"), "connection reset"),
            &observed,
        );

        let _ = stream.next_event().await;
    }
}

/// A client built `connect`: where it reaches a listener, the listener-only
/// setting `through_server` cannot reach, and each refusal of what a real
/// socket fixes for itself.
#[cfg(feature = "server")]
mod connected {
    use std::{
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use crate::{
        http::{Response, body::Body},
        router::service::Service,
        server::{Running, Server},
        test::{TestClient, reachable},
    };

    fn document() -> kynos_openapi::Document {
        kynos_openapi::Document::new(
            kynos_openapi::SpecVersion::V3_1,
            kynos_openapi::Info::new("Test", "1"),
        )
    }

    /// Answers every request with an empty 200.
    fn answering() -> Service<()> {
        Service::new(document(), |_| async { Response::new(Body::empty()) })
    }

    async fn spawned(server: Server<()>) -> Running<()> {
        server
            .bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .prepare()
            .await
            .expect("loopback listener binds")
            .spawn()
    }

    fn address(written: &str) -> SocketAddr {
        written.parse().expect("a socket address")
    }

    /// An unspecified address names no host to connect to, so its family's
    /// loopback stands in; any other is reached as it is.
    #[test]
    fn an_unspecified_address_is_reached_through_its_loopback() {
        assert_eq!(reachable(address("0.0.0.0:80")), address("127.0.0.1:80"));
        assert_eq!(reachable(address("[::]:80")), address("[::1]:80"));
        assert_eq!(reachable(address("192.0.2.7:80")), address("192.0.2.7:80"));
        assert_eq!(reachable(address("[::1]:80")), address("[::1]:80"));
    }

    /// The connection limit is the listener's: with a cap of one, a second
    /// request waits in the backlog while the first is served, and is served
    /// once the first ends.
    #[cfg(feature = "http1")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_connection_limit_holds_a_second_request_until_the_first_ends() {
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let service = {
            let calls = Arc::clone(&calls);
            let release = Arc::clone(&release);
            Service::new(document(), move |_| {
                let calls = Arc::clone(&calls);
                let release = Arc::clone(&release);
                async move {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        release.notified().await;
                    }
                    Response::new(Body::empty())
                }
            })
        };
        let running =
            spawned(Server::new(service).max_connections(std::num::NonZeroUsize::MIN)).await;
        let client = Arc::new(TestClient::connect(&running));
        let send = || {
            let client = Arc::clone(&client);
            tokio::spawn(async move { client.get("/").send().await.status() })
        };
        let served = |count| {
            let calls = Arc::clone(&calls);
            async move {
                while calls.load(Ordering::SeqCst) != count {
                    tokio::task::yield_now().await;
                }
            }
        };

        let first = send();
        tokio::time::timeout(Duration::from_secs(5), served(1))
            .await
            .expect("the first request reaches the handler");
        let second = send();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), served(2))
                .await
                .is_err(),
            "the second connection must wait in the listener backlog"
        );

        release.notify_one();
        assert_eq!(first.await.expect("the first request joins"), 200);
        assert_eq!(second.await.expect("the second request joins"), 200);
        running
            .shutdown()
            .await
            .expect("an idle server drains cleanly");
    }

    #[tokio::test]
    #[should_panic(
        expected = "a client built `connect` sends from its own socket's address, which \
                    `with_peer` cannot set"
    )]
    async fn a_connected_client_refuses_a_default_peer() {
        let running = spawned(Server::new(answering())).await;
        let _ = TestClient::connect(&running).with_peer(address("203.0.113.7:4242"));
    }

    #[tokio::test]
    #[should_panic(
        expected = "a client built `connect` sends from its own socket's address, which \
                    `peer` cannot set"
    )]
    async fn a_connected_client_refuses_a_requests_peer() {
        let running = spawned(Server::new(answering())).await;
        let _ = TestClient::connect(&running)
            .get("/")
            .peer(address("203.0.113.7:4242"));
    }

    #[tokio::test]
    #[should_panic(expected = "a client built `connect` holds a body to its server's \
                               `request_body_idle_timeout`; set it there")]
    async fn a_connected_client_refuses_an_idle_timeout_of_its_own() {
        let running = spawned(Server::new(answering())).await;
        let _ = TestClient::connect(&running).with_request_body_idle_timeout(None);
    }

    #[tokio::test]
    #[should_panic(expected = "GET /: a client built `connect` cannot time on a paused clock")]
    async fn a_connected_client_refuses_to_time_on_a_paused_clock() {
        let running = spawned(Server::new(answering())).await;
        let _ = TestClient::connect(&running).get("/").send_timed().await;
    }

    #[cfg(feature = "tls")]
    #[tokio::test]
    #[should_panic(expected = "a server whose listeners speak TLS cannot be connected to")]
    async fn a_server_speaking_tls_cannot_be_connected_to() {
        let certified = rcgen::generate_simple_self_signed(["localhost".to_owned()])
            .expect("a self-signed certificate");
        let tls = crate::server::tls::TlsConfig::from_pem(
            certified.cert.pem().as_bytes(),
            certified.signing_key.serialize_pem().as_bytes(),
        )
        .expect("TLS identity parses");
        let running = spawned(Server::new(answering()).tls(tls)).await;
        let _ = TestClient::connect(&running);
    }

    #[cfg(unix)]
    #[tokio::test]
    #[should_panic(expected = "a server with no TCP listener cannot be connected to")]
    async fn a_server_with_no_tcp_listener_cannot_be_connected_to() {
        let directory = std::env::temp_dir().join(format!("kynos-{}-connect", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let listener = std::os::unix::net::UnixListener::bind(directory.join("api.sock"))
            .expect("the socket binds");
        let running = Server::new(answering())
            .unix_listener(listener)
            .prepare()
            .await
            .expect("a Unix listener alone is something to listen on")
            .spawn();
        let _ = std::fs::remove_dir_all(&directory);
        let _ = TestClient::connect(&running);
    }
}
