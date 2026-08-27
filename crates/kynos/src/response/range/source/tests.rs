use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use super::{ByteSource, InMemory, SPAN, Spans, Truncated};
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

/// Polls `body` to a stop, into the octets it yielded and whatever ended it.
///
/// Takes the body by reference so a test can ask what it reports *after* it has
/// stopped, which is where both the exhaustion and the truncation cases are
/// decided.
async fn collect<S: ByteSource>(
    body: &mut Spans<S>,
) -> (Vec<u8>, Option<crate::http::body::BoxError>) {
    use std::{
        future::poll_fn,
        pin::Pin,
        task::{Poll, ready},
    };

    use http_body::Body;

    let mut collected = Vec::new();
    let mut failure = None;
    poll_fn(|context| {
        loop {
            match ready!(Pin::new(&mut *body).poll_frame(context)) {
                Some(Ok(frame)) => {
                    if let Ok(span) = frame.into_data() {
                        collected.extend_from_slice(&span);
                    }
                }
                Some(Err(error)) => {
                    failure = Some(error);
                    return Poll::Ready(());
                }
                None => return Poll::Ready(()),
            }
        }
    })
    .await;
    (collected, failure)
}

/// Drains a body that cannot fail into the octets it yielded.
async fn drain<S: ByteSource>(mut body: Spans<S>) -> Vec<u8> {
    let (collected, failure) = collect(&mut body).await;
    assert!(
        failure.is_none(),
        "this source cannot fail: {}",
        failure.map(|error| error.to_string()).unwrap_or_default()
    );
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

/// A source that stops early fails the body rather than ending it short.
///
/// The head is already on the wire by the time a span is read, carrying a
/// `Content-Length` -- and a `Content-Range` for a 206 -- sized from the
/// complete length. Ending the body here would answer 200 or 206 with fewer
/// octets than those fields name, which is the truncation section 14.4 tells a
/// recipient never to recombine. It has to be a stream failure instead.
#[tokio::test]
async fn a_source_that_returns_nothing_fails_the_body() {
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

    let mut body = Spans::new(Arc::new(Empty), 0, 999);
    let (octets, failure) = collect(&mut body).await;

    assert!(octets.is_empty());
    let failure = failure.expect("a source that reads nothing cannot fill the length it named");
    let truncated = failure
        .downcast_ref::<Truncated>()
        .expect("the failure names the span the source would not fill");
    assert_eq!((truncated.first(), truncated.last()), (0, 999));
    assert!(
        failure.to_string().contains("0..=999"),
        "the message says which offsets went unanswered: {failure}"
    );

    // Terminating rather than spinning was the whole reason the old code ended
    // the body, so the replacement owes the same property: a driver that polls
    // on past the failure is ended, not read from again.
    let (after, failure) = collect(&mut body).await;
    assert!(after.is_empty());
    assert!(
        failure.is_none(),
        "the failure is reported once, not forever"
    );
}

/// A source that answers with less than it was asked for is asked again.
///
/// The other half of the same decision. A short read makes progress, so the
/// remainder is read on the next poll and the span is filled in full -- there
/// is nothing wrong on the wire to fail over, and failing would rule out every
/// source that answers with whatever one underlying read returned.
#[tokio::test]
async fn a_short_read_is_finished_on_the_next_poll() {
    /// Answers a kibibyte at a time however wide the ask.
    struct Trickling {
        reads: AtomicUsize,
    }

    impl ByteSource for Trickling {
        type Error = std::convert::Infallible;

        async fn complete_length(&self) -> Result<u64, Self::Error> {
            Ok(4_096)
        }

        async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let width = usize::try_from(last - first + 1).unwrap().min(1_024);
            Ok(Bytes::from(vec![b'y'; width]))
        }
    }

    let source = Arc::new(Trickling {
        reads: AtomicUsize::new(0),
    });

    let mut body = Spans::new(Arc::clone(&source), 0, 4_095);
    let (octets, failure) = collect(&mut body).await;

    assert!(failure.is_none(), "a short read is not a failure");
    assert_eq!(
        octets.len(),
        4_096,
        "every octet the head named is delivered"
    );
    assert_eq!(
        source.reads.load(Ordering::Relaxed),
        4,
        "one read per kibibyte the source was willing to answer with"
    );
}

/// An exhausted body owes nothing.
///
/// The span is inclusive, so the count remaining is one more than the
/// difference -- which is one, not zero, once the cursor has passed the last
/// offset. A `Content-Length` an octet too long is one a client waits out.
#[tokio::test]
async fn an_exhausted_body_reports_no_remaining_octets() {
    use http_body::Body;

    let mut body = Spans::new(Counting::new(4), 0, 3);
    assert_eq!(body.size_hint().exact(), Some(4));

    let (octets, failure) = collect(&mut body).await;

    assert!(failure.is_none());
    assert_eq!(octets.len(), 4);
    assert_eq!(
        body.size_hint().exact(),
        Some(0),
        "a drained body still naming an octet is one nothing will ever send"
    );
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
