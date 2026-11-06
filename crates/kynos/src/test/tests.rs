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
