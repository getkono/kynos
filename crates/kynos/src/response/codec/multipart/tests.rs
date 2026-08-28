use super::{BOUNDARY_PREFIX, Part, boundary, contains, quoted, render, unfolded};

/// Reads a rendered body back with `multer`.
///
/// The independently constructed oracle a parser owes. `multer` never saw
/// how `render` writes a body, so agreement between the two is evidence
/// about the format rather than about one implementation — where reading it
/// back with Kynos's own extractor would only prove that the writer and the
/// reader share a misunderstanding.
async fn reparse(body: bytes::Bytes, boundary: &str) -> Vec<Part> {
    let mut fields = multer::Multipart::new(Once(Some(Ok(body))), boundary);
    let mut parts = Vec::new();

    while let Some(field) = fields.next_field().await.expect("a well-formed body") {
        parts.push(Part {
            name: field.name().expect("a named part").to_owned(),
            file_name: field.file_name().map(str::to_owned),
            content_type: field.content_type().map(ToString::to_string),
            bytes: field.bytes().await.expect("readable part bytes"),
        });
    }

    parts
}

/// A stream yielding one item and then ending.
///
/// Hand-written for the reason `tests/sse.rs` gives: a stream combinator
/// crate as a new dev-dependency reworks the UI snapshots that embed
/// rustc's "the following other types implement" list.
struct Once(Option<Result<bytes::Bytes, std::convert::Infallible>>);

impl futures_core::Stream for Once {
    type Item = Result<bytes::Bytes, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.0.take())
    }
}

fn part(name: &str, bytes: &[u8]) -> Part {
    Part {
        name: name.to_owned(),
        file_name: None,
        content_type: None,
        bytes: bytes::Bytes::copy_from_slice(bytes),
    }
}

/// Every part Kynos writes is read back as the part it was.
#[tokio::test]
async fn every_part_survives_a_round_trip_through_an_independent_reader() {
    let parts = vec![
        part("plain", b"a value"),
        Part {
            name: "avatar".to_owned(),
            file_name: Some("portrait.png".to_owned()),
            content_type: Some("image/png".to_owned()),
            bytes: bytes::Bytes::from_static(&[0x89, b'P', b'N', b'G', 0x00, 0xff]),
        },
        Part {
            name: "note".to_owned(),
            file_name: None,
            content_type: Some("text/plain; charset=utf-8".to_owned()),
            bytes: bytes::Bytes::from_static("héllo — ✓".as_bytes()),
        },
        // Empty, which is a part rather than an absence.
        part("empty", b""),
        // Bytes that look like framing, which must not frame anything.
        part("tricky", b"\r\n--not-a-boundary\r\n\r\n"),
    ];

    let delimiter = boundary(&parts);
    let body = render(parts.clone(), &delimiter);

    assert_eq!(reparse(body, &delimiter).await, parts);
}

/// A name carrying the one character a quoted-string escapes still comes
/// back as itself.
#[tokio::test]
async fn a_name_needing_escapes_survives_a_round_trip() {
    let parts = vec![Part {
        name: r#"od"d name"#.to_owned(),
        file_name: Some(r#"a "quoted" file.txt"#.to_owned()),
        content_type: None,
        bytes: bytes::Bytes::from_static(b"x"),
    }];

    let delimiter = boundary(&parts);
    let body = render(parts.clone(), &delimiter);

    assert_eq!(reparse(body, &delimiter).await, parts);
}

/// A backslash is dropped rather than escaped, and the name still parses.
///
/// The lossy case, pinned in both halves: what is written, and that a name
/// *ending* in a backslash -- the input that made the whole header
/// unparseable when it was escaped -- reads back cleanly now.
#[tokio::test]
async fn a_backslash_is_dropped_rather_than_written_unreadably() {
    assert_eq!(quoted(r"od\d"), "odd".to_owned());

    let parts = vec![Part {
        name: r"trailing\".to_owned(),
        file_name: None,
        content_type: None,
        bytes: bytes::Bytes::from_static(b"x"),
    }];

    let delimiter = boundary(&parts);
    let body = render(parts, &delimiter);
    let read_back = reparse(body, &delimiter).await;

    assert_eq!(read_back.len(), 1);
    assert_eq!(read_back[0].name, "trailing".to_owned());
}

/// The delimiter is raised until no part encapsulates it, which RFC 2046
/// requires and which Kynos has no randomness to make merely likely.
#[test]
fn the_delimiter_is_one_no_part_contains() {
    let first = format!("{BOUNDARY_PREFIX}{:016x}", 0);
    let parts = vec![part("adversarial", first.as_bytes())];

    let chosen = boundary(&parts);

    assert_ne!(chosen, first);
    assert!(!contains(&parts[0].bytes, chosen.as_bytes()));
}

/// A body written to defeat the search still gets a delimiter it does not
/// hold, however many candidates that takes.
#[test]
fn the_search_passes_every_candidate_a_body_encapsulates() {
    let adversarial: Vec<u8> = (0..4)
        .flat_map(|counter| format!("{BOUNDARY_PREFIX}{counter:016x}").into_bytes())
        .collect();
    let parts = vec![part("adversarial", &adversarial)];

    let chosen = boundary(&parts);

    assert_eq!(chosen, format!("{BOUNDARY_PREFIX}{:016x}", 4));
}

/// A line ending in a name would end the header rather than appear in it,
/// so it is dropped rather than escaped.
#[test]
fn a_line_ending_cannot_reach_a_header_value() {
    assert_eq!(
        quoted("name\r\nX-Injected: yes"),
        "nameX-Injected: yes".to_owned()
    );
    assert_eq!(
        unfolded("text/plain\r\n X-Injected: yes"),
        "text/plain X-Injected: yes".to_owned()
    );
}

/// The body ends with the closing delimiter and nothing after it: a
/// preamble and an epilogue are both legal and both ignored, so writing
/// either would be bytes every recipient discards.
#[test]
fn a_body_carries_no_preamble_and_no_epilogue() {
    let parts = vec![part("one", b"x")];
    let delimiter = boundary(&parts);
    let body = render(parts, &delimiter);

    assert!(body.starts_with(format!("--{delimiter}\r\n").as_bytes()));
    assert!(body.ends_with(format!("--{delimiter}--\r\n").as_bytes()));
}
