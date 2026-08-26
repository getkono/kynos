//! Writing `multipart/form-data` as a response.
//!
//! The declared field names, per-part media types and encodings are preserved
//! in both directions, so a `MultipartForm<T>` returned from a handler
//! describes the same parts it would accept.
//!
//! `multer` parses; nothing writes. So the body is rendered here, to RFC 7578
//! over RFC 2046: a `Content-Disposition` naming the field and the part's own
//! `Content-Type` when it declared one. The delimiters around them belong to
//! RFC 2046 rather than to this subtype and come from `response::framing`,
//! which the two multipart subtypes share. A plain `Serialize` bound
//! would not do — a [`FilePart`]'s bytes would reach the wire as an array of
//! numbers — so the writing half has a trait of its own, mirroring the reading
//! one.

use bytes::Bytes;

use crate::{
    extract::{
        body::multipart::{FilePart, MultipartForm, Part},
        describe::RequestContent,
    },
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses, framing},
    schema::{Schema, registry::Registry},
};

// The framing is RFC 2046's rather than this subtype's, so the delimiter search
// and the unfolding moved out with it and the byteranges writer under
// `response::range` uses the same ones.
use crate::response::framing::unfolded;
// The search's own vocabulary, which only this file's tests still name.
#[cfg(test)]
use crate::response::framing::{BOUNDARY_PREFIX, contains};

/// How a declared type becomes the parts of a `multipart/form-data` body.
///
/// The counterpart of
/// [`FromMultipart`](crate::extract::body::multipart::FromMultipart), yielding
/// the same [`Part`]s it consumes. `#[derive(MultipartForm)]` writes both from
/// one declaration, which is what makes "the same parts it would accept" true
/// by construction.
///
/// ```
/// use kynos::{
///     extract::body::multipart::{FilePart, Part},
///     response::codec::multipart::IntoMultipart,
/// };
///
/// struct Avatar(FilePart);
///
/// impl IntoMultipart for Avatar {
///     fn into_parts(self) -> Vec<Part> {
///         vec![Part {
///             name: "avatar".to_owned(),
///             file_name: self.0.file_name,
///             content_type: self.0.content_type,
///             bytes: self.0.bytes,
///         }]
///     }
/// }
/// ```
pub trait IntoMultipart {
    /// Renders the value as the parts of a body, in the order they are written.
    fn into_parts(self) -> Vec<Part>;
}

/// How one declared field becomes the part that carries it.
///
/// The counterpart of
/// [`FromPart`](crate::extract::body::multipart::FromPart), implemented for the
/// same three shapes a form field takes. An `Option<T>` field writes nothing
/// when it is absent and a `Vec<T>` field writes one part per element, so an
/// implementation here only ever produces a single part.
pub trait IntoPart {
    /// Renders the field as one part carried under `name`.
    fn into_part(self, name: &str) -> Part;
}

impl IntoPart for FilePart {
    fn into_part(self, name: &str) -> Part {
        Part {
            name: name.to_owned(),
            file_name: self.file_name,
            content_type: self.content_type,
            bytes: self.bytes,
        }
    }
}

/// Typeless bytes, which is the default RFC 7578 derives for a part whose
/// schema states no type — the same default the describing half relies on.
impl IntoPart for Bytes {
    fn into_part(self, name: &str) -> Part {
        Part {
            name: name.to_owned(),
            file_name: None,
            content_type: Some("application/octet-stream".to_owned()),
            bytes: self,
        }
    }
}

/// The charset is stated rather than left to RFC 7578's `text/plain` default,
/// because Kynos writes and reads UTF-8 and a recipient guessing otherwise
/// would decode a different string than the one that was sent.
impl IntoPart for String {
    fn into_part(self, name: &str) -> Part {
        Part {
            name: name.to_owned(),
            file_name: None,
            content_type: Some("text/plain; charset=utf-8".to_owned()),
            bytes: Bytes::from(self.into_bytes()),
        }
    }
}

impl<T: IntoMultipart> IntoResponse for MultipartForm<T> {
    fn into_response(self) -> Response {
        let parts = self.0.into_parts();
        let boundary = boundary(&parts);
        let body = render(parts, &boundary);

        let mut response = Response::new(Body::from_bytes(body));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::try_from(format!("multipart/form-data; boundary={boundary}"))
                .expect("a generated boundary is printable ASCII"),
        );
        response
    }
}

/// A delimiter no part contains, over the octets these parts carry.
fn boundary(parts: &[Part]) -> String {
    framing::boundary(parts.iter().map(|part| part.bytes.as_ref()))
}

/// The body: this subtype's header block per part, framed by RFC 2046.
fn render(parts: Vec<Part>, boundary: &str) -> Bytes {
    let encapsulations = parts
        .into_iter()
        .map(|part| (headers(&part), part.bytes))
        .collect();

    framing::render(encapsulations, boundary)
}

/// The header lines one form-data part declares, CRLF-terminated.
///
/// RFC 7578's whole contribution to the framing: which field this part is, the
/// file name if it had one, and the media type if it declared one.
fn headers(part: &Part) -> Vec<u8> {
    let mut headers = Vec::with_capacity(128);

    headers.extend_from_slice(b"Content-Disposition: form-data; name=\"");
    headers.extend_from_slice(quoted(&part.name).as_bytes());
    headers.push(b'"');
    if let Some(file_name) = &part.file_name {
        headers.extend_from_slice(b"; filename=\"");
        headers.extend_from_slice(quoted(file_name).as_bytes());
        headers.push(b'"');
    }
    headers.extend_from_slice(framing::CRLF);

    if let Some(content_type) = &part.content_type {
        headers.extend_from_slice(b"Content-Type: ");
        headers.extend_from_slice(unfolded(content_type).as_bytes());
        headers.extend_from_slice(framing::CRLF);
    }

    headers
}

/// A `Content-Disposition` parameter, as the quoted-string it travels in.
///
/// RFC 7578 carries the field name and the file name as quoted-strings and says
/// to send them as UTF-8, so the text itself is left alone and only what a
/// quoted-string cannot hold is touched.
///
/// Three characters cannot be held. A `"` is escaped, because that is the one
/// escape every reader of this format performs. A line ending would end the
/// header rather than appear in it, so it is dropped: a name that spans two
/// lines is a name that would inject a third party's header.
///
/// A `\` is dropped for the same reason as a line ending — it cannot be
/// represented, only misread. Escaping it as `\\` produces a name readers
/// return with both backslashes, including
/// [`multer`](https://docs.rs/multer), the reader Kynos itself uses on the
/// extracting half; and a name *ending* in one makes the whole header
/// unparseable, since the scan for the closing quote treats the escape as
/// covering it and runs off the end. Percent-encoding it, which is what the
/// HTML form algorithm does for `"`, only moves the problem: nothing on the
/// reading side percent-decodes. Dropping is the one option under which every
/// name Kynos writes is a name Kynos reads back.
fn quoted(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => {
                quoted.push('\\');
                quoted.push('"');
            }
            '\\' | '\r' | '\n' => {}
            _ => quoted.push(character),
        }
    }
    quoted
}

impl<T: Schema> Responses for MultipartForm<T> {
    // Taken from the extracting half rather than rebuilt, which is what makes
    // "the same parts it would accept" true by construction instead of by
    // agreement between two lists of parts and encodings.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut response = kynos_openapi::Response::new("OK");
        response.content = <Self as RequestContent>::request_body(registry).content;

        kynos_openapi::Responses::new().with(200, response)
    }
}

#[cfg(test)]
mod tests {
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
}
