//! The request body a `TestRequest` sends: what it yields, and what it tells a
//! reader about its end and its length before the reader polls. And, in
//! [`assertions`], how each `TestResponse::assert_*` helper fails.
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
}
