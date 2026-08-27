use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use super::{ByteSource, InMemory, SPAN, Spans};
use bytes::Bytes;

/// A source that records what it was asked for.
///
/// The point of the whole abstraction is *what is read*, not what comes back,
/// so the fake counts spans and reports the widest one it saw.
struct Counting {
    length: u64,
    reads: AtomicUsize,
    widest: AtomicU64,
}

impl Counting {
    fn new(length: u64) -> Arc<Self> {
        Arc::new(Self {
            length,
            reads: AtomicUsize::new(0),
            widest: AtomicU64::new(0),
        })
    }
}

impl ByteSource for Counting {
    type Error = std::convert::Infallible;

    async fn complete_length(&self) -> Result<u64, Self::Error> {
        Ok(self.length)
    }

    async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.widest.fetch_max(last - first + 1, Ordering::Relaxed);
        Ok(Bytes::from(vec![
            b'x';
            usize::try_from(last - first + 1).unwrap()
        ]))
    }
}

/// Drains a body into the octets it yielded.
async fn drain<S: super::ByteSource>(mut body: super::Spans<S>) -> Vec<u8> {
    use std::{
        future::poll_fn,
        pin::Pin,
        task::{Poll, ready},
    };

    use http_body::Body;

    let mut collected = Vec::new();
    poll_fn(|context| {
        loop {
            match ready!(Pin::new(&mut body).poll_frame(context)) {
                Some(Ok(frame)) => {
                    if let Ok(span) = frame.into_data() {
                        collected.extend_from_slice(&span);
                    }
                }
                Some(Err(error)) => panic!("this source cannot fail: {error}"),
                None => return Poll::Ready(()),
            }
        }
    })
    .await;
    collected
}

/// The whole representation is delivered without ever being held at once.
///
/// The requirement in as many words: an acceptance contract asking for
/// "incremental asynchronous reads with backpressure and cancellation; the full
/// file MUST never be buffered" is not satisfied by a source that reads
/// everything and slices. Asserted by the widest span the source was asked for,
/// which is the only thing that distinguishes the two implementations from
/// outside.
#[tokio::test]
async fn a_whole_representation_is_read_one_span_at_a_time() {
    let length = SPAN * 4 + 17;
    let source = Counting::new(length);

    let octets = drain(Spans::new(Arc::clone(&source), 0, length - 1)).await;

    assert_eq!(octets.len(), usize::try_from(length).unwrap());
    assert_eq!(
        source.widest.load(Ordering::Relaxed),
        SPAN,
        "a span wider than the configured one means the body was read whole"
    );
    assert_eq!(source.reads.load(Ordering::Relaxed), 5);
}

/// A part costs a part.
///
/// The other half of the same property, and the reason a range request exists:
/// serving a kilobyte out of a gigabyte reads a kilobyte.
#[tokio::test]
async fn a_part_reads_only_the_part() {
    let source = Counting::new(SPAN * 1024);

    let octets = drain(Spans::new(Arc::clone(&source), 10, 1_033)).await;

    assert_eq!(octets.len(), 1_024);
    assert_eq!(source.reads.load(Ordering::Relaxed), 1);
    assert_eq!(source.widest.load(Ordering::Relaxed), 1_024);
}

/// A single-octet representation is still a representation.
#[tokio::test]
async fn one_octet_is_one_span() {
    let source = Counting::new(1);
    assert_eq!(drain(Spans::new(source, 0, 0)).await.len(), 1);
}

/// A source that stops early ends the body rather than spinning on it.
#[tokio::test]
async fn a_source_that_returns_nothing_ends_the_body() {
    struct Empty;

    impl ByteSource for Empty {
        type Error = std::convert::Infallible;

        async fn complete_length(&self) -> Result<u64, Self::Error> {
            Ok(1_000)
        }

        async fn read_span(&self, _first: u64, _last: u64) -> Result<Bytes, Self::Error> {
            Ok(Bytes::new())
        }
    }

    assert!(drain(Spans::new(Arc::new(Empty), 0, 999)).await.is_empty());
}

/// The in-memory source answers what it holds, and clamps rather than panicking.
#[tokio::test]
async fn the_in_memory_source_slices_what_it_has() {
    let source = InMemory::new(Bytes::from_static(b"0123456789"));

    assert_eq!(source.complete_length().await.unwrap(), 10);
    assert_eq!(source.read_span(2, 4).await.unwrap(), &b"234"[..]);
    // Past the end selects fewer octets, matching `Rangeable::slice`.
    assert_eq!(source.read_span(8, 99).await.unwrap(), &b"89"[..]);
    assert!(source.read_span(99, 200).await.unwrap().is_empty());
}
