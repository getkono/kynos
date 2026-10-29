use std::time::Duration;

use super::{Event, heartbeat_record};
use crate::{http::body::BoxError, response::stream::records::RecordBuffer};

/// One event, written as the first record of a stream of its own.
fn encode<T: serde::Serialize>(event: &Event<T>) -> Result<bytes::Bytes, BoxError> {
    RecordBuffer::new().record(|record| super::encode(record, &mut Vec::new(), event))
}

/// Re-parses a record into its `(field, value)` pairs, using a reader
/// transcribed from the `text/event-stream` grammar rather than from
/// [`encode`].
///
/// An oracle derived from the writer would agree with it by construction,
/// including wherever both are wrong — which is the whole of the Parser rule
/// in `docs/testing.md`.
fn reparse(record: &[u8]) -> Vec<(String, String)> {
    let text = std::str::from_utf8(record).expect("a UTF-8 record");
    let mut fields = Vec::new();

    // `end-of-line = ( cr lf / cr / lf )`: a CRLF is one break, and a CR on
    // its own is as much a break as an LF.
    for line in text.replace("\r\n", "\n").split(['\r', '\n']) {
        if line.is_empty() {
            // The blank line that dispatches the event.
            continue;
        }

        // The format splits on the first colon; one space after it is part
        // of the delimiter rather than of the value.
        let (name, value) = line.split_once(':').expect("a `name: value` line");
        fields.push((
            name.to_owned(),
            value.strip_prefix(' ').unwrap_or(value).to_owned(),
        ));
    }

    fields
}

#[test]
fn a_keep_alive_comment_is_framed_as_a_record_a_client_ignores() {
    let record = heartbeat_record("ping");

    // An empty field name is the comment form, which every client discards.
    assert_eq!(reparse(&record), [(String::new(), "ping".to_owned())]);
    assert!(
        record.ends_with(b"\n\n"),
        "a record ends with the blank line that dispatches it"
    );
}

/// A line break inside a value would otherwise end the field, so a
/// multi-line comment travels as several fields of the same name.
#[test]
fn a_multi_line_keep_alive_comment_is_written_one_line_per_field() {
    let record = heartbeat_record("first\nsecond");

    assert_eq!(
        reparse(&record),
        [
            (String::new(), "first".to_owned()),
            (String::new(), "second".to_owned()),
        ]
    );
}

/// The default carries no text, which is the shortest thing a client will
/// still read as a live connection.
#[test]
fn a_keep_alive_with_no_comment_is_still_a_record() {
    let record = heartbeat_record("");

    assert_eq!(&record[..], b": \n\n");
}

/// The same framing an event's own fields take, so a comment and an event
/// cannot disagree about what a record is.
#[test]
fn an_event_ends_with_the_blank_line_that_dispatches_it() {
    let record = encode(&Event::new(1_u8)).expect("an encodable event");

    assert!(record.ends_with(b"\n\n"));
    assert_eq!(reparse(&record), [("data".to_owned(), "1".to_owned())]);
}

/// Every field an event can carry, in the order the format wants them.
///
/// `data` last is not cosmetic: a client dispatches on the blank line and
/// reads the other fields as belonging to the data it has accumulated, so
/// an `id` written after `data` belongs to the *next* event.
#[test]
fn an_event_writes_every_field_it_carries_in_dispatch_order() {
    let event = Event::new(vec![1_u8, 2])
        .comment("about to happen")
        .event("created")
        .id("42")
        .retry(Duration::from_secs(3));

    assert_eq!(
        reparse(&encode(&event).expect("an encodable event")),
        [
            (String::new(), "about to happen".to_owned()),
            ("event".to_owned(), "created".to_owned()),
            ("id".to_owned(), "42".to_owned()),
            ("retry".to_owned(), "3000".to_owned()),
            ("data".to_owned(), "[1,2]".to_owned()),
        ]
    );
}

/// The format carries `retry` as an integer of milliseconds, so a finer
/// `Duration` is rounded up to whole milliseconds rather than truncated or
/// written as a fraction a client would refuse.
#[test]
fn a_retry_is_written_in_whole_milliseconds_rounding_the_remainder_up() {
    let event = Event::new(0_u8).retry(Duration::from_micros(1_001));
    let fields = reparse(&encode(&event).expect("an encodable event"));

    assert_eq!(fields[0], ("retry".to_owned(), "2".to_owned()));
}

/// Truncating a non-zero delay under a millisecond would write `retry: 0`,
/// which tells a client to reconnect at once.
#[test]
fn a_sub_millisecond_retry_is_never_written_as_zero() {
    let event = Event::new(0_u8).retry(Duration::from_nanos(1));
    let fields = reparse(&encode(&event).expect("an encodable event"));

    assert_eq!(fields[0], ("retry".to_owned(), "1".to_owned()));
}

/// An explicit zero is the one value that already says what it means.
#[test]
fn a_zero_retry_is_written_as_zero() {
    let event = Event::new(0_u8).retry(Duration::ZERO);
    let fields = reparse(&encode(&event).expect("an encodable event"));

    assert_eq!(fields[0], ("retry".to_owned(), "0".to_owned()));
}

/// An omitted field is absent rather than empty: a client reads `id:` with
/// no value as *clearing* the last event id, which is not what an event
/// that never set one means.
#[test]
fn an_event_writes_no_field_it_did_not_carry() {
    let record = encode(&Event::new(1_u8)).expect("an encodable event");
    let written: Vec<String> = reparse(&record).into_iter().map(|(name, _)| name).collect();

    assert_eq!(written, ["data"]);
}

/// A newline inside a value is how the format carries a newline at all:
/// several fields of one name, which a client rejoins with `\n`.
///
/// This is where a golden-string snapshot would be worthless -- it would
/// pin the bytes `encode` happens to write, and the question is whether a
/// reader recovers the value.
#[test]
fn a_multi_line_value_travels_as_one_field_per_line() {
    let event = Event::new("first\nsecond\nthird");
    let fields = reparse(&encode(&event).expect("an encodable event"));

    // JSON keeps the breaks inside the string, so the data is one line.
    assert_eq!(
        fields,
        [("data".to_owned(), r#""first\nsecond\nthird""#.to_owned())]
    );

    // A comment is not JSON, so its breaks do reach the framing.
    let event = Event::new(0_u8).comment("first\nsecond");
    assert_eq!(
        reparse(&encode(&event).expect("an encodable event")),
        [
            (String::new(), "first".to_owned()),
            (String::new(), "second".to_owned()),
            ("data".to_owned(), "0".to_owned()),
        ]
    );
}

/// A CRLF in a value is one line break, not two.
///
/// The format ends a line on CR, LF or CRLF, so writing the CR through
/// would produce a stray empty field -- and an empty field is the blank
/// line that dispatches the event, which would cut the record in half.
#[test]
fn a_carriage_return_does_not_become_a_second_line() {
    let event = Event::new(0_u8).comment("first\r\nsecond");

    assert_eq!(
        reparse(&encode(&event).expect("an encodable event")),
        [
            (String::new(), "first".to_owned()),
            (String::new(), "second".to_owned()),
            ("data".to_owned(), "0".to_owned()),
        ]
    );
}

/// A bare CR ends a line as surely as an LF does, so it ends the field.
///
/// Written through, it would end the field on the client anyway and start a
/// new one from whatever follows -- an `id` taken from user data could set
/// `retry`, name the event, or inject `data`.
#[test]
fn a_bare_carriage_return_ends_the_field_rather_than_starting_another() {
    let event = Event::new(0_u8)
        .comment("note\r: forged")
        .event("created\rdata: forged")
        .id("a\rretry: 1");
    let record = encode(&event).expect("an encodable event");

    assert_eq!(
        reparse(&record),
        [
            (String::new(), "note".to_owned()),
            (String::new(), ": forged".to_owned()),
            ("event".to_owned(), "created".to_owned()),
            ("event".to_owned(), "data: forged".to_owned()),
            ("id".to_owned(), "a".to_owned()),
            ("id".to_owned(), "retry: 1".to_owned()),
            ("data".to_owned(), "0".to_owned()),
        ]
    );
    assert!(
        !record.contains(&b'\r'),
        "every line ends with the LF the writer chose"
    );

    let record = heartbeat_record("ping\rdata: forged");
    assert_eq!(
        reparse(&record),
        [
            (String::new(), "ping".to_owned()),
            (String::new(), "data: forged".to_owned()),
        ]
    );
}

/// A CR at the end of a value is a break like any other, not half of a
/// CRLF that never arrives, so it writes the same fields an LF there does.
///
/// Dropped instead, `id("a\r")` would leave a client's last event ID at `a`
/// where `id("a\n")` resets it to empty.
#[test]
fn a_trailing_carriage_return_breaks_the_value_as_a_line_feed_does() {
    let carriage_return = Event::new(0_u8).comment("x\r\r").id("a\r");
    let line_feed = Event::new(0_u8).comment("x\n\n").id("a\n");

    assert_eq!(
        reparse(&encode(&carriage_return).expect("an encodable event")),
        [
            (String::new(), "x".to_owned()),
            (String::new(), String::new()),
            (String::new(), String::new()),
            ("id".to_owned(), "a".to_owned()),
            ("id".to_owned(), String::new()),
            ("data".to_owned(), "0".to_owned()),
        ]
    );
    assert_eq!(
        encode(&carriage_return).expect("an encodable event"),
        encode(&line_feed).expect("an encodable event")
    );
}

/// An event whose data cannot be serialized is an error rather than a
/// record, because a half-written record would desynchronize the stream.
#[test]
fn an_unserializable_event_is_refused_rather_than_half_written() {
    use std::collections::HashMap;

    // A map keyed by something JSON cannot spell as an object key.
    let mut data = HashMap::new();
    data.insert(vec![1_u8], "value");

    assert!(encode(&Event::new(data)).is_err());
}

/// The events of one stream share a block and a data buffer, so each record
/// has to be its own fields and nothing an earlier one wrote: not its data,
/// not the fields it carried, and not the half of the data a refused event
/// serialized before it failed.
#[test]
fn each_event_of_a_stream_is_written_as_its_own_record() {
    use std::collections::HashMap;

    let mut records = RecordBuffer::new();
    let mut data = Vec::new();
    let mut write = |event: &Event<serde_json::Value>| {
        records.record(|record| super::encode(record, &mut data, event))
    };

    let first = write(
        &Event::new(serde_json::json!([1, 2, 3]))
            .id("1")
            .comment("hello"),
    )
    .expect("an encodable event");
    let second = write(&Event::new(serde_json::json!(4))).expect("an encodable event");

    // Its data is half serialized when the map key is refused.
    let mut unwritable = HashMap::new();
    unwritable.insert(vec![1_u8], "value");
    let refused = records.record(|record| {
        super::encode(
            record,
            &mut data,
            &Event::new(("partial", unwritable)).event("broken"),
        )
    });
    assert!(refused.is_err());

    let third = records
        .record(|record| super::encode(record, &mut data, &Event::new(5_u8)))
        .expect("an encodable event");

    assert_eq!(&first[..], b": hello\nid: 1\ndata: [1,2,3]\n\n");
    assert_eq!(&second[..], b"data: 4\n\n");
    assert_eq!(&third[..], b"data: 5\n\n");
}

/// A zero interval re-arms the keep-alive timer at the instant it fired, so an
/// idle stream would write heartbeat records back to back. It is refused where
/// it is configured rather than discovered on a live connection.
#[test]
#[should_panic(expected = "a keep-alive interval must be greater than zero")]
fn a_zero_keep_alive_interval_is_refused() {
    let _ = super::KeepAlive::new().interval(Duration::ZERO);
}

/// An event stream answers as `text/event-stream`, whatever its keep-alive.
///
/// On a runtime, since a keep-alive's timer is created with the response.
#[tokio::test]
async fn an_event_stream_is_sent_as_text_event_stream() {
    use crate::response::IntoResponse;

    for sse in [
        super::Sse::new(Ticking::new(0, Duration::ZERO)),
        super::Sse::new(Ticking::new(0, Duration::ZERO)).keep_alive(super::KeepAlive::new()),
    ] {
        let response = sse.into_response();

        assert_eq!(
            response
                .headers()
                .get_all(crate::http::header::CONTENT_TYPE)
                .iter()
                .collect::<Vec<_>>(),
            ["text/event-stream"]
        );
    }
}

/// Yields `remaining` events, one every `gap`, then ends.
struct Ticking {
    remaining: u8,
    gap: Duration,
    timer: std::pin::Pin<Box<tokio::time::Sleep>>,
}

impl Ticking {
    fn new(remaining: u8, gap: Duration) -> Self {
        Self {
            remaining,
            gap,
            timer: Box::pin(tokio::time::sleep(gap)),
        }
    }
}

impl futures_core::Stream for Ticking {
    type Item = Result<Event<u8>, std::convert::Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.remaining == 0 {
            return std::task::Poll::Ready(None);
        }

        std::task::ready!(this.timer.as_mut().poll(context));
        this.remaining -= 1;
        let next = tokio::time::Instant::now() + this.gap;
        this.timer.as_mut().reset(next);

        std::task::Poll::Ready(Some(Ok(Event::new(this.remaining))))
    }
}

/// The records a stream of four events `gap` apart sends under a 100 ms
/// keep-alive, `true` for each heartbeat and `false` for each event.
async fn records_under_keep_alive(gap: Duration) -> Vec<bool> {
    use http_body_util::BodyExt as _;

    use crate::response::IntoResponse;

    let mut body = super::Sse::new(Ticking::new(4, gap))
        .keep_alive(super::KeepAlive::new().interval(Duration::from_millis(100)))
        .into_response()
        .into_body();
    let mut records = Vec::new();

    while let Some(frame) = body.frame().await {
        let data = frame.expect("a record").into_data().expect("a data frame");
        records.push(data.starts_with(b":"));
    }

    records
}

/// Every event restarts the keep-alive, so a stream whose events come more
/// often than the interval never sends one: here every 60 ms under a 100 ms
/// keep-alive, which a timer left running would interrupt at 100 and 200.
#[tokio::test(start_paused = true)]
async fn an_event_restarts_the_keep_alive() {
    assert_eq!(
        records_under_keep_alive(Duration::from_millis(60)).await,
        [false; 4]
    );
}

/// The control: events 150 ms apart leave each 100 ms silence one heartbeat
/// long, so a heartbeat precedes every event.
#[tokio::test(start_paused = true)]
async fn a_silence_longer_than_the_interval_sends_a_keep_alive() {
    assert_eq!(
        records_under_keep_alive(Duration::from_millis(150)).await,
        [true, false, true, false, true, false, true, false]
    );
}
