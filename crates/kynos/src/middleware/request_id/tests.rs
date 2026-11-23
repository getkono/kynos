use std::{
    future::Future,
    pin::{Pin, pin},
    sync::{Arc, atomic::AtomicU64},
    task::{Context, Poll, Waker},
};

use super::{AssignedId, CorrelationHeaders, Counter, Digits, Random, RequestId, RequestIdSource};
use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{HeaderName, HeaderValue, Request, Response, StatusCode, body::Body},
    middleware::{
        Next,
        erased::{ErasedInterceptor, ErasedTerminal},
    },
};

/// Answers with the identifier the request's `AssignedId` holds, under
/// `seen`, or with no such field where it holds none.
struct Echo;

impl ErasedTerminal<()> for Echo {
    fn call<'a>(
        &'a self,
        request: Request,
        (): &'a (),
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        let seen = AssignedId::of(&request).map(|id| id.as_header_value().clone());
        Box::pin(async move {
            let mut response = Response::new(Body::empty());
            if let Some(seen) = seen {
                response
                    .headers_mut()
                    .insert(HeaderName::from_static("seen"), seen);
            }
            response
        })
    }
}

/// What `layer` and then [`Echo`] answer a bare request with. Nothing in the
/// chain waits, so one poll settles it.
fn served(layer: Arc<dyn ErasedInterceptor<()>>) -> Response {
    let layers = [layer];
    let future = Next::new(&layers, &Echo, &(), None).run(Request::new(Body::empty()));
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(continued) => continued.into_response(),
        Poll::Pending => panic!("nothing in the chain waits"),
    }
}

/// A group whose one name is not the default's, so a reader that matched
/// `x-request-id` would read nothing.
struct Correlation(HeaderValue);

impl HeaderParams for Correlation {
    const NAMES: &'static [&'static str] = &["x-correlation-id"];
}

impl EncodeHeaders for Correlation {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        Ok(vec![(
            HeaderName::from_static("x-correlation-id"),
            self.0.clone(),
        )])
    }
}

impl CorrelationHeaders for Correlation {
    fn from_id(id: HeaderValue) -> Self {
        Self(id)
    }
}

/// A group that refuses to encode any identifier.
struct Refusing;

impl HeaderParams for Refusing {
    const NAMES: &'static [&'static str] = &["x-request-id"];
}

impl EncodeHeaders for Refusing {
    fn encode(&self) -> Result<Vec<(HeaderName, HeaderValue)>, UnencodableHeader> {
        Err(UnencodableHeader::new(HeaderName::from_static(
            "x-request-id",
        )))
    }
}

impl CorrelationHeaders for Refusing {
    fn from_id(id: HeaderValue) -> Self {
        let _ = id;
        Self
    }
}

/// The chain inside sees, and the response outside carries, the identifier
/// the group's own header does, whichever name that is.
#[test]
fn the_assigned_identifier_is_the_one_the_header_carries_at_both_ends() {
    let response = served(Arc::new(RequestId::new().header::<Correlation>()));

    let carried = response
        .headers()
        .get("x-correlation-id")
        .expect("the group's header");
    assert_eq!(response.headers().get("seen"), Some(carried));
    assert_eq!(
        AssignedId::of_response(&response).map(AssignedId::as_header_value),
        Some(carried)
    );
    assert_eq!(
        RequestId::of_response(&response),
        AssignedId::of_response(&response)
    );
}

/// A group that refuses to encode answers a 500 without the chain and without
/// a header, so no identifier is handed on either.
#[test]
fn a_refused_group_assigns_no_identifier() {
    let response = served(Arc::new(RequestId::new().header::<Refusing>()));

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers().get("seen"), None);
    assert_eq!(AssignedId::of_response(&response), None);
}

/// Text where the identifier is visible ASCII, and `None` where it is not.
#[test]
fn an_identifier_is_text_only_where_it_is_visible_ascii() {
    assert_eq!(
        AssignedId(HeaderValue::from_static("abc")).as_str(),
        Some("abc")
    );
    let opaque = HeaderValue::from_bytes(b"caf\xe9").expect("obs-text is a field value");
    assert_eq!(AssignedId(opaque).as_str(), None);
}

/// A counter identifier is its count in decimal, with no leading zero, at
/// both ends of the range and where a digit carries.
#[test]
fn a_counter_identifier_is_its_count_in_decimal() {
    for n in [0, 9, 10, 1_000, u64::MAX] {
        let source = Counter {
            next: AtomicU64::new(n),
        };

        assert_eq!(source.next_id(), n.to_string().as_str());
    }
}

/// The identifier is the two halves in order, each zero-padded to sixteen
/// digits.
#[test]
fn a_random_identifier_writes_its_halves_in_order() {
    let source = Random::new();
    let [high, low] = source.halves(0);

    assert_eq!(source.next_id(), format!("{high:016x}{low:016x}").as_str());
}

/// A value shorter than its width is zero-padded to it, so a random
/// identifier whose high half starts with a zero keeps all 32 digits.
#[test]
fn digits_pad_a_short_value_to_its_width() {
    assert_eq!(
        Digits::<32>::value::<16>(1, 32),
        "00000000000000000000000000000001"
    );
    assert_eq!(
        Digits::<32>::value::<16>(u128::from(u64::MAX), 32),
        "0000000000000000ffffffffffffffff"
    );
    assert_eq!(Digits::<32>::value::<16>(0, 32), "0".repeat(32).as_str());
}

/// An identifier is 128 bits written as 32 lowercase hex digits, leading
/// zeros included, so every identifier has the same length.
#[test]
fn a_random_identifier_is_thirty_two_lowercase_hex_digits() {
    let source = Random::new();

    for _ in 0..64 {
        let id = source.next_id();
        let id = id.to_str().expect("hex digits are visible ASCII");

        assert_eq!(id.len(), 32, "{id}");
        assert!(
            id.bytes()
                .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f')),
            "{id}"
        );
    }
}

/// The counter behind the hash never reaches the wire: successive identifiers
/// differ, and none is the counter's own value.
#[test]
fn successive_random_identifiers_differ() {
    let source = Random::new();
    let ids: std::collections::HashSet<String> = (0..1024)
        .map(|_| source.next_id().to_str().expect("hex digits").to_owned())
        .collect();

    assert_eq!(ids.len(), 1024);
    for n in 0..1024_u32 {
        assert!(!ids.contains(format!("{n:032x}").as_str()), "{n} leaked");
    }
}

/// Two sources stand for two processes: the same count maps to different
/// identifiers, which is what makes an identifier unique across a restart.
#[test]
fn two_random_sources_map_one_count_apart() {
    let (first, second) = (Random::new(), Random::new());

    assert_ne!(first.halves(0), second.halves(0));
    assert_ne!(first.next_id(), second.next_id());
}

/// The two halves are hashed apart, so neither repeats the other.
#[test]
fn a_random_identifier_does_not_repeat_its_half() {
    let source = Random::new();
    let [high, low] = source.halves(7);

    assert_ne!(high, low);
}
