use super::{Event, encode, heartbeat_record};

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

    for line in text.split('\n') {
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
        .retry(3_000);

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
