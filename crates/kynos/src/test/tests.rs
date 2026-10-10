//! The request body a `TestRequest` sends: what it yields, and what it tells a
//! reader about its end and its length before the reader polls.
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
