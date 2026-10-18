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

use super::{Framed, RequestBody};

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
        http::{HeaderName, HeaderValue, Response, StatusCode, body::Body},
        test::TestResponse,
    };

    /// A received response, as `TestRequest::send` leaves one.
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
        TestResponse { response, body }
    }

    #[test]
    #[should_panic(expected = "expected 200 OK, received 404 Not Found\nbody: gone")]
    fn assert_status_fails_on_another_status_and_shows_the_body() {
        received(StatusCode::NOT_FOUND, &[], b"gone").assert_status(StatusCode::OK);
    }

    #[test]
    #[should_panic(expected = "the body is not a problem document")]
    fn assert_problem_type_fails_on_a_body_that_is_not_json() {
        received(StatusCode::BAD_REQUEST, &[], b"plain").assert_problem_type("about:blank");
    }

    #[test]
    #[should_panic(expected = "the problem document's `type` is not a string")]
    fn assert_problem_type_fails_on_a_type_that_is_not_a_string() {
        received(StatusCode::BAD_REQUEST, &[], br#"{"type":7}"#).assert_problem_type("about:blank");
    }

    /// An omitted `type` is `about:blank` (RFC 9457 section 3.1.1), and is
    /// reported as that rather than as an absence.
    #[test]
    #[should_panic(expected = "expected problem type `urn:example:x`, received `about:blank`")]
    fn assert_problem_type_fails_on_another_type_reading_an_omitted_one_as_blank() {
        received(StatusCode::BAD_REQUEST, &[], br#"{"status":400}"#)
            .assert_problem_type("urn:example:x");
    }

    #[test]
    #[should_panic(expected = "`x-trace` is not on the response")]
    fn assert_header_fails_on_an_absent_field() {
        received(StatusCode::OK, &[], b"").assert_header("x-trace", "1");
    }

    #[test]
    #[should_panic(expected = "expected `x-trace: 1`, received `x-trace: 2`")]
    fn assert_header_fails_on_another_value() {
        received(StatusCode::OK, &[("x-trace", "2")], b"").assert_header("x-trace", "1");
    }

    #[test]
    #[should_panic(expected = "no `Set-Cookie` names `session`; the response set [\"theme\"]")]
    fn assert_cookie_fails_on_a_cookie_never_set_and_names_the_ones_that_were() {
        received(StatusCode::OK, &[("set-cookie", "theme=dark; Path=/")], b"")
            .assert_cookie("session", "s");
    }

    #[test]
    #[should_panic(expected = "expected cookie `theme=light`, received `theme=dark`")]
    fn assert_cookie_fails_on_another_value() {
        received(StatusCode::OK, &[("set-cookie", "theme=dark; Path=/")], b"")
            .assert_cookie("theme", "light");
    }

    #[test]
    #[should_panic(expected = "expected a redirect, received 200 OK")]
    fn assert_redirect_fails_on_a_status_that_is_not_a_redirect() {
        received(StatusCode::OK, &[("location", "/next")], b"").assert_redirect("/next");
    }

    #[test]
    #[should_panic(expected = "expected `location: /next`, received `location: /elsewhere`")]
    fn assert_redirect_fails_on_another_location() {
        received(StatusCode::SEE_OTHER, &[("location", "/elsewhere")], b"")
            .assert_redirect("/next");
    }

    #[test]
    #[should_panic(expected = "expected 206, received 200 OK")]
    fn assert_part_fails_on_a_status_other_than_206() {
        received(StatusCode::OK, &[("content-range", "bytes 0-1/4")], b"ab").assert_part(0, 1, 4);
    }

    #[test]
    #[should_panic(
        expected = "expected `content-range: bytes 0-1/4`, received `content-range: bytes 0-2/4`"
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
    #[should_panic(expected = "`Content-Range` names 2 octet(s) and the body carries 3")]
    fn assert_part_fails_on_a_body_the_field_does_not_describe() {
        received(
            StatusCode::PARTIAL_CONTENT,
            &[("content-range", "bytes 0-1/4")],
            b"abc",
        )
        .assert_part(0, 1, 4);
    }
}
