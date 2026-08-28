use http_body_util::BodyExt;

use std::sync::{Arc, Mutex};

use super::{Body, Bytes, Delivery, HttpBody};

/// A recorder for the one report a watched body makes.
///
/// A `Mutex<Vec<_>>` rather than a single slot, so that a second report --
/// the failure the once-only rule exists to stop -- is visible as a second
/// entry rather than overwriting the first.
#[derive(Clone, Default)]
struct Reports(Arc<Mutex<Vec<Delivery>>>);

impl Reports {
    /// Watches `body`, recording how it ends.
    fn watching(&self, body: Body) -> Body {
        let reports = Arc::clone(&self.0);
        body.watching(move |delivery| {
            reports
                .lock()
                .expect("an unpoisoned recorder")
                .push(delivery);
        })
    }

    /// What was reported, in order.
    fn taken(&self) -> Vec<Delivery> {
        self.0.lock().expect("an unpoisoned recorder").clone()
    }
}

/// Reads a body to the bytes it carries, driving `poll_frame` to its end.
async fn drain(body: Body) -> Bytes {
    body.collect()
        .await
        .expect("a body built from bytes cannot fail")
        .to_bytes()
}

#[tokio::test]
async fn an_empty_body_carries_no_bytes() {
    assert!(drain(Body::empty()).await.is_empty());
}

#[tokio::test]
async fn a_body_carries_exactly_the_bytes_it_was_given() {
    let bytes = Bytes::from_static(br#"{"id":1}"#);
    assert_eq!(drain(Body::from_bytes(bytes.clone())).await, bytes);
}

#[tokio::test]
async fn the_default_body_is_the_empty_one() {
    assert!(drain(Body::default()).await.is_empty());
}

/// Both answers come from a lock rather than from the erased body directly,
/// so they are worth asking before anything has read a frame -- which is
/// when a `Content-Length` is decided.
#[test]
fn an_empty_body_states_its_end_and_its_length() {
    let body = Body::empty();

    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
}

#[test]
fn a_body_states_its_length_before_it_is_read() {
    let body = Body::from_bytes(Bytes::from_static(b"1234"));

    assert!(!body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(4));
}

/// Watching must not change what a body is: a wrapper that lost the exact
/// length would silently switch every watched response to chunked framing.
#[test]
fn a_watched_body_states_the_length_the_body_beneath_it_states() {
    let reports = Reports::default();
    let body = reports.watching(Body::from_bytes(Bytes::from_static(b"1234")));

    assert_eq!(body.size_hint().exact(), Some(4));
    assert!(!body.is_end_stream());
}

#[tokio::test]
async fn a_watched_body_read_to_its_end_reports_delivery_once() {
    let reports = Reports::default();
    let body = reports.watching(Body::from_bytes(Bytes::from_static(b"1234")));

    assert_eq!(drain(body).await, Bytes::from_static(b"1234"));
    assert_eq!(reports.taken(), vec![Delivery::Complete]);
}

/// The signal this exists for. The bytes were there and nothing read them,
/// which from the peer's side is a response that never arrived.
#[tokio::test]
async fn a_watched_body_dropped_before_its_end_reports_an_interruption() {
    let reports = Reports::default();

    drop(reports.watching(Body::from_bytes(Bytes::from_static(b"1234"))));

    assert_eq!(reports.taken(), vec![Delivery::Interrupted]);
}

/// The pass control for the case above, differing in exactly one property:
/// there was nothing to deliver. An empty response is the ordinary one, and
/// reporting every 204 as an interruption would make the signal useless.
#[tokio::test]
async fn a_watched_empty_body_reports_delivery_even_unpolled() {
    let reports = Reports::default();

    drop(reports.watching(Body::empty()));

    assert_eq!(reports.taken(), vec![Delivery::Complete]);
}

/// Once, not twice. The drop runs after the read that already reported, and
/// a duplicate would double-count every completed response.
#[tokio::test]
async fn a_watched_body_reports_once_across_both_of_its_ends() {
    let reports = Reports::default();
    let body = reports.watching(Body::from_bytes(Bytes::from_static(b"1234")));

    let _ = drain(body).await;

    assert_eq!(
        reports.taken(),
        vec![Delivery::Complete],
        "the drop reported a second time over the read that had already reported"
    );
}
