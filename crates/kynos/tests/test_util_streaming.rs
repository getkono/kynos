//! A response body read as it arrives, through `TestRequest::send_streaming`.
//!
//! One reason: a stream that never ends on its own cannot be asserted on
//! through `send`, which returns only once the body has. Each case runs over
//! both transports, since a stream is a `Service` body in one and a hyper
//! `Incoming` on its own connection in the other, and a hang-up means
//! something different to each.

#![cfg(all(
    feature = "openapi32",
    feature = "macros",
    feature = "server",
    feature = "http1",
    feature = "test-util"
))]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use kynos::{
    Router,
    di::inject::Inject,
    http::StatusCode,
    response::{
        status::NoContent,
        stream::sse::{Event, Sse},
    },
    server::Server,
    test::{Coverage, TestClient},
};
use tokio::sync::{mpsc, oneshot};

/// The ticks the test sends, and the signal the operation's stream gives when
/// it is released, handed to the one request that opens the feed.
#[derive(Default)]
struct Hub {
    pending: Mutex<Option<Ticks>>,
}

/// An event per tick the test sends, until the test stops sending. Holds the
/// sending half of `released`, so dropping the stream is what fires it.
struct Ticks {
    ticks: mpsc::UnboundedReceiver<u32>,
    _released: oneshot::Sender<()>,
}

impl futures_core::Stream for Ticks {
    type Item = Result<Event<u32>, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.ticks
            .poll_recv(context)
            .map(|tick| tick.map(|tick| Ok(Event::new(tick).event("tick"))))
    }
}

#[kynos::get("/feed")]
async fn feed(Inject(hub): Inject<Arc<Hub>>) -> Sse<Ticks> {
    Sse::new(
        hub.pending
            .lock()
            .expect("an unpoisoned hub")
            .take()
            .expect("one request opens the feed"),
    )
}

#[kynos::get("/ping")]
async fn ping() -> NoContent {
    NoContent
}

/// What the test holds of the feed: the ticks it sends, and the signal that
/// the operation's stream was released.
struct Feed {
    ticks: mpsc::UnboundedSender<u32>,
    released: oneshot::Receiver<()>,
}

/// A hub with one feed waiting, and the test's end of it.
fn hub() -> (Arc<Hub>, Feed) {
    let (ticks, receiver) = mpsc::unbounded_channel();
    let (signal, released) = oneshot::channel();
    let hub = Hub {
        pending: Mutex::new(Some(Ticks {
            ticks: receiver,
            _released: signal,
        })),
    };
    (Arc::new(hub), Feed { ticks, released })
}

fn service(hub: Arc<Hub>) -> kynos::router::service::Service<Arc<Hub>> {
    Router::<Arc<Hub>>::new()
        .mount(kynos::routes![feed, ping])
        .build(hub)
        .expect("a describable router")
}

/// A client of each transport over a fresh hub, with the test's end of its
/// feed and the transport's name.
fn clients() -> [(&'static str, TestClient<Arc<Hub>>, Feed); 2] {
    let (direct, direct_feed) = hub();
    let (served, served_feed) = hub();
    [
        ("direct", TestClient::new(service(direct)), direct_feed),
        (
            "served",
            TestClient::through_server(Server::new(service(served))),
            served_feed,
        ),
    ]
}

/// The case the method exists for: read an event, act while the stream is
/// open, read the next, and see the end once the operation's stream ends.
#[tokio::test]
async fn events_are_read_as_they_arrive_and_the_client_stays_usable() {
    for (transport, client, feed) in clients() {
        let mut stream = client.get("/feed").send_streaming().await;
        assert_eq!(stream.status(), StatusCode::OK, "{transport}");
        assert_eq!(
            stream.header("content-type"),
            Some("text/event-stream"),
            "{transport}"
        );

        feed.ticks.send(1).expect("an open feed");
        let first = stream.next_event().await.expect("the first tick");
        assert_eq!(first.event.as_deref(), Some("tick"), "{transport}");
        assert_eq!(first.data, "1", "{transport}");

        // The same client answers another request while the stream is open.
        client.get("/ping").send().await.assert_status(204);

        feed.ticks.send(2).expect("an open feed");
        assert_eq!(
            stream.next_event().await.map(|event| event.data),
            Some("2".to_owned()),
            "{transport}"
        );

        drop(feed.ticks);
        assert_eq!(stream.next_event().await, None, "{transport}");
    }
}

/// Dropping the stream is a client hanging up: the operation's stream is
/// released, which it was not while the test held the stream open.
///
/// Through a server the departure is learnt at the next write, as over a
/// socket, so a tick after the drop is what reveals it; in process the drop
/// releases it outright, and the tick finds nothing to send to.
#[tokio::test]
async fn dropping_the_stream_releases_the_operations_stream() {
    for (transport, client, mut feed) in clients() {
        let mut stream = client.get("/feed").send_streaming().await;
        feed.ticks.send(1).expect("an open feed");
        assert!(stream.next_event().await.is_some(), "{transport}");
        assert_eq!(
            feed.released.try_recv(),
            Err(oneshot::error::TryRecvError::Empty),
            "{transport}: released while the test still held it open"
        );

        drop(stream);
        let _ = feed.ticks.send(2);

        let released = tokio::time::timeout(Duration::from_secs(5), feed.released)
            .await
            .unwrap_or_else(|_| panic!("{transport}: the stream outlived the hang-up"));
        assert!(
            released.is_err(),
            "{transport}: released means dropped, never sent"
        );
    }
}

/// An exchange is recorded once its head arrives, so a stream dropped unread
/// counts toward coverage, by the client and by a shared `Coverage`, and its
/// head conforms without a body anything could judge.
#[tokio::test]
async fn a_stream_dropped_unread_is_recorded_from_its_head() {
    for (_, client, _feed) in clients() {
        let coverage = Coverage::new();
        let client = client.with_coverage(&coverage);

        drop(client.get("/feed").send_streaming().await);
        client.get("/ping").send().await.assert_status(204);

        client.assert_conformance();
        client.assert_declared_responses_covered();
        coverage.assert_declared_responses_covered(service(hub().0).openapi());
    }
}

/// `finish` reads the rest and returns the whole response: the events already
/// read are still among its events, and its body is checked for conformance.
#[tokio::test]
async fn finish_returns_the_whole_response() {
    for (transport, client, feed) in clients() {
        let mut stream = client.get("/feed").send_streaming().await;
        feed.ticks.send(1).expect("an open feed");
        assert!(stream.next_event().await.is_some(), "{transport}");
        feed.ticks.send(2).expect("an open feed");
        drop(feed.ticks);

        let response = stream.finish().await;

        response.assert_status(200);
        let ticks: Vec<_> = response
            .events()
            .into_iter()
            .map(|event| event.data)
            .collect();
        assert_eq!(ticks, ["1", "2"], "{transport}");
        client.assert_conformance();
    }
}
