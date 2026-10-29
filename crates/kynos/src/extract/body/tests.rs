use kynos_openapi::Schema as OpenApiSchema;

use crate::{
    extract::{FromRequest, body::binary::Binary, describe::RequestContent},
    http::{
        Request,
        media::{OctetStream, Png},
    },
    schema::registry::Registry,
};

/// Extracts `T` from `request`, as a handler's last argument is.
async fn extract<T: FromRequest<()>>(request: Request) -> Result<T, T::Rejection> {
    let (mut parts, body) = request.into_parts();
    T::from_request(&mut parts, body, &()).await
}

/// Raw binary is described by what is *left out*, which is easy to get wrong in
/// the direction of saying too much.
///
/// `type` is absent because raw binary sits outside the types JSON Schema
/// describes, and `contentMediaType` is absent because it would only repeat the
/// key the content sits under. What remains is the empty Schema Object.
#[test]
fn a_raw_binary_body_states_no_type_and_repeats_no_media_type() {
    let body = Binary::<Png>::request_body(&mut Registry::default());

    let content = body
        .content
        .get("image/png")
        .and_then(kynos_openapi::RefOr::as_item)
        .expect("the body is keyed by its own media type");

    let Some(OpenApiSchema::Object(schema)) = &content.schema else {
        panic!("expected a keyword-carrying schema rather than a boolean or nothing");
    };
    assert_eq!(schema.ty, None, "raw binary is outside `type`");
    assert_eq!(
        schema.format, None,
        "`format: binary` is the OpenAPI 3.0 spelling and is deprecated"
    );
    assert_eq!(
        schema.content_media_type, None,
        "a `contentMediaType` repeating the content key is redundant, and a \
         contradicting one is ignored by the specification"
    );
    assert_eq!(schema.content_encoding, None, "these bytes are not encoded");
}

/// The media type reaches the description from the marker, so two markers
/// produce two different keys from one type.
#[test]
fn the_marker_chooses_the_content_key() {
    let png = Binary::<Png>::request_body(&mut Registry::default());
    let bytes = Binary::<OctetStream>::request_body(&mut Registry::default());

    assert!(png.content.contains_key("image/png"));
    assert!(bytes.content.contains_key("application/octet-stream"));
}

/// A `Binary` marker beside another body is refused exactly when `offers`
/// hands some request to both: a request need not repeat a marker's
/// parameters, so the body's own bare media type reaches a marker naming its
/// type and subtype whatever parameters it declares. Case and surrounding
/// whitespace aside, only a different type or subtype keeps the pair, and a
/// structured suffix is a different subtype.
#[test]
fn a_marker_is_refused_wherever_a_request_reaches_both_sides() {
    use crate::extract::body::{alternative::shares_requests, offers};
    use crate::http::{HeaderMap, HeaderValue, header};

    let pairs = [
        ("application/json", "application/json", true),
        ("Application/JSON", "application/json", true),
        ("application/json; charset=utf-8", "application/json", true),
        (
            "application/json;charset=\"UTF-8\";",
            "application/json",
            true,
        ),
        (" application/json ; flag", "application/json", true),
        (" application/json ;q=1", "application/json", true),
        ("application/json; version=2", "application/json", true),
        ("text/plain", "text/plain", true),
        ("text/plain; charset=iso-8859-1", "text/plain", true),
        ("application/vnd.x+json", "application/json", false),
        ("application/jsonl", "application/json", false),
        ("application/json-seq", "application/json", false),
        ("image/png", "application/json", false),
        // Whitespace is ASCII on both sides: a no-break space is part of
        // the type, so it keeps the pair rather than vanishing in `offers`.
        ("\u{a0}application/json", "application/json", false),
        ("application/json\u{2003}", "application/json", false),
    ];
    #[cfg(feature = "multipart")]
    let pairs = pairs.into_iter().chain([(
        "multipart/form-data; boundary=x",
        "multipart/form-data",
        true,
    )]);

    for (marker, body, refused) in pairs {
        assert_eq!(
            shares_requests(marker, body),
            refused,
            "`{marker}` beside `{body}`"
        );

        // Held to `offers`: the pair is refused exactly when one of the
        // probes, the marker's own value, the body's, or either with a
        // charset, reaches both sides, where tuple order would pick the side.
        let essence = marker.split(';').next().unwrap_or(marker).trim();
        let probes = [
            marker.to_owned(),
            body.to_owned(),
            format!("{essence}; charset=utf-8"),
            format!("{essence}; charset=iso-8859-1"),
        ];
        let shared = probes.iter().any(|probe| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(probe).expect("a header value"),
            );
            offers(&headers, marker) && offers(&headers, body)
        });
        assert_eq!(shared, refused, "`offers` on `{marker}` beside `{body}`");
    }
}

/// The constant `OneOf` evaluates refuses a pair `shares_requests` finds
/// sharing one. Its `compile_fail` doctests run outside nextest, so this calls
/// the check at run time, where its refusal is a panic.
#[test]
#[should_panic(expected = "a `Binary` marker is offered requests the body beside it")]
fn a_shared_marker_is_refused_by_the_constant_one_of_evaluates() {
    crate::extract::body::alternative::distinct("text/plain; charset=iso-8859-1", "text/plain");
}

/// A parameter a marker declares is the value a request may carry for it, or
/// none: `charset=utf-8`, which every codec otherwise accepts, does not reach
/// a Latin-1 marker, and a charset's name is compared without regard to case
/// (RFC 9110 section 8.3.2).
#[test]
fn a_declared_parameter_governs_the_value_a_request_carries() {
    use crate::extract::body::offers;
    use crate::http::{HeaderMap, HeaderValue, header};

    for (marker, content_type, offered) in [
        ("text/plain; charset=iso-8859-1", "text/plain", true),
        (
            "text/plain; charset=iso-8859-1",
            "text/plain; charset=iso-8859-1",
            true,
        ),
        (
            "text/plain; charset=iso-8859-1",
            "text/plain; charset=\"ISO-8859-1\"",
            true,
        ),
        (
            "text/plain; charset=iso-8859-1",
            "text/plain; charset=utf-8",
            false,
        ),
        ("text/html; charset=utf-8", "text/html; charset=UTF-8", true),
        (
            "text/html; charset=utf-8",
            "text/html; charset=iso-8859-1",
            false,
        ),
        (
            "application/vnd.x; version=2",
            "application/vnd.x; version=2; charset=utf-8",
            true,
        ),
        (
            "application/vnd.x; version=2",
            "application/vnd.x; version=3",
            false,
        ),
        // A `;` inside a quoted value separates nothing (RFC 9110 section 5.6.6).
        (
            "application/vnd.x+json; profile=\"a;b\"",
            "application/vnd.x+json; profile=\"a;b\"",
            true,
        ),
        (
            "application/vnd.x+json; profile=\"a;b\"",
            "application/vnd.x+json",
            true,
        ),
        (
            "application/vnd.x+json; profile=\"a;b\"",
            "application/vnd.x+json; profile=\"a;b\"; charset=utf-8",
            true,
        ),
        (
            "application/vnd.x+json; profile=\"a;b\"",
            "application/vnd.x+json; profile=\"a\"",
            false,
        ),
        (
            "application/vnd.x+json; profile=\"a\"",
            "application/vnd.x+json; profile=\"a;b\"",
            false,
        ),
        (
            "application/vnd.x+json; profile=\"a;version=2\"",
            "application/vnd.x+json; version=2",
            false,
        ),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        assert_eq!(
            offers(&headers, marker),
            offered,
            "`{content_type}` to `{marker}`"
        );
    }
}

/// The documented remedy for a variable number of uploads has to compile.
///
/// `multipart.rs` tells a reader to declare one field of type `Vec<FilePart>`,
/// and `MultipartForm<T>` requires `T: Schema` — so a `FilePart` without one
/// makes the only advice the module gives impossible to take.
#[cfg(feature = "multipart")]
mod a_file_part_describes_itself {
    use kynos_openapi::Schema as OpenApiSchema;

    use crate::{
        extract::body::multipart::FilePart,
        schema::{Schema, registry::Registry},
    };

    /// A part's bytes are raw binary, and its media type belongs to the
    /// Encoding Object rather than to the schema — where a `contentMediaType`
    /// would contradict it and be ignored.
    #[test]
    fn a_part_is_raw_binary() {
        let OpenApiSchema::Object(schema) = FilePart::schema(&mut Registry::default()) else {
            panic!("expected a keyword-carrying schema rather than a boolean");
        };
        assert_eq!(schema.ty, None, "raw binary is outside `type`");
        assert_eq!(schema.format, None, "`format: binary` is the 3.0 spelling");
        assert_eq!(
            schema.content_media_type, None,
            "the Encoding Object carries the part's media type"
        );
    }

    /// An anonymous schema is inlined; a part has no component name to `$ref`.
    #[test]
    fn a_part_is_not_a_component() {
        assert!(FilePart::name().is_none());
    }
}

/// A file field keeps everything its part carried but the name, which the
/// declaring field records, and a `Bytes` field keeps the octets alone.
#[cfg(feature = "multipart")]
#[test]
fn a_file_part_keeps_what_its_part_carried() {
    use bytes::Bytes;

    use crate::extract::body::multipart::{FilePart, FromPart, Part};

    let part = Part {
        name: "avatar".to_owned(),
        file_name: Some("a.png".to_owned()),
        content_type: Some("image/png".to_owned()),
        bytes: Bytes::from_static(b"\x89PNG"),
    };
    let file = FilePart {
        file_name: Some("a.png".to_owned()),
        content_type: Some("image/png".to_owned()),
        bytes: Bytes::from_static(b"\x89PNG"),
    };

    assert_eq!(FilePart::from(part.clone()), file);
    assert_eq!(FilePart::from_part(part.clone()).expect("any part"), file);
    assert_eq!(Bytes::from_part(part).expect("any part"), &b"\x89PNG"[..]);
}

/// Describing a body sets the operation's request body to the one its type
/// states: keyed by the single media type it lists and reads, over the schema
/// of what it decodes.
mod a_body_describes_the_media_type_it_reads {
    use kynos_openapi::{RefOr, Schema as OpenApiSchema};

    use crate::{
        extract::{
            body::{binary::Binary, text::Text},
            describe::{Describe, RequestContent},
        },
        http::media::Png,
        router::operation::OperationCx,
        schema::registry::Registry,
    };

    fn describes<T: Describe + RequestContent>(media_type: &str, schema: OpenApiSchema) {
        assert_eq!(T::media_types(), [media_type]);

        let mut registry = Registry::default();
        let mut operation = OperationCx::new(&mut registry);
        T::describe(&mut operation);
        let Some(RefOr::Item(body)) = operation.finish().request_body else {
            panic!("describing the body declares no request body");
        };

        assert_eq!(body, T::request_body(&mut Registry::default()));
        assert_eq!(
            body.content.keys().map(String::as_str).collect::<Vec<_>>(),
            [media_type]
        );
        assert_eq!(
            body.content[media_type].as_item().expect("inline").schema,
            Some(schema)
        );
    }

    #[test]
    fn as_binary() {
        describes::<Binary<Png>>("image/png", OpenApiSchema::Object(Box::default()));
    }

    #[test]
    fn as_text() {
        describes::<Text>("text/plain", Registry::default().resolve::<String>());
    }

    #[cfg(feature = "form")]
    #[test]
    fn as_a_form() {
        describes::<crate::extract::body::form::Form<u32>>(
            "application/x-www-form-urlencoded",
            Registry::default().resolve::<u32>(),
        );
    }

    #[cfg(feature = "multipart")]
    #[test]
    fn as_multipart() {
        describes::<crate::extract::body::multipart::MultipartForm<u32>>(
            "multipart/form-data",
            Registry::default().resolve::<u32>(),
        );
    }

    #[cfg(feature = "protobuf")]
    #[test]
    fn as_protobuf() {
        describes::<crate::extract::body::protobuf::Protobuf<u32>>(
            "application/protobuf",
            Registry::default().resolve::<u32>(),
        );
    }
}

/// A `String` field taking over its part's octets must not take over the rest
/// of the buffer they were sliced from: a short field would keep the whole
/// request body alive for as long as the value lives.
#[cfg(feature = "multipart")]
mod a_text_part_holds_only_its_own_octets {
    use bytes::Bytes;

    use crate::extract::body::multipart::{FromPart, Part};

    fn part(bytes: Bytes) -> Part {
        Part {
            name: "a".to_owned(),
            file_name: None,
            content_type: None,
            bytes,
        }
    }

    /// The last reference to a larger buffer, as a decoded part is once the
    /// body around it is dropped.
    #[test]
    fn a_sole_slice_of_a_larger_buffer_is_released() {
        let mut buffer = Vec::with_capacity(4096);
        buffer.extend_from_slice(b"--x\r\nhi");
        let body = Bytes::from(buffer);
        let octets = body.slice(5..);
        drop(body);

        let text = String::from_part(part(octets)).expect("UTF-8");
        assert_eq!(text, "hi");
        assert_eq!(text.capacity(), text.len());
    }

    /// Octets something else still holds are copied at their length.
    #[test]
    fn a_shared_slice_is_copied_at_its_length() {
        let body = Bytes::from(b"--x\r\nhi".to_vec());
        let text = String::from_part(part(body.slice(5..))).expect("UTF-8");
        assert_eq!(text, "hi");
        assert_eq!(text.capacity(), text.len());
        assert_eq!(&body[..], b"--x\r\nhi");
    }
}

/// Any `ParamValue` is a text field, read with `FromStr` as a parameter is,
/// and text it refuses is a 422 at the part's name carrying `FromStr`'s own
/// error, as is a part that is no text at all.
#[cfg(feature = "multipart")]
mod a_param_value_part_is_read_as_its_text {
    use std::collections::BTreeMap;

    use bytes::Bytes;

    use crate::{
        error::rejection::BodyRejection,
        extract::body::multipart::{FromPart, Part},
    };

    fn part(octets: &'static [u8]) -> Part {
        Part {
            name: "count".to_owned(),
            file_name: None,
            content_type: None,
            bytes: Bytes::from_static(octets),
        }
    }

    /// The 422's failures, keyed by JSON Pointer.
    fn failures<T: std::fmt::Debug>(result: Result<T, BodyRejection>) -> BTreeMap<String, String> {
        let rejection = result.expect_err("a refused part");
        let BodyRejection::Schema { failures } = rejection else {
            panic!("expected a 422, got {rejection:?}");
        };
        failures
    }

    fn at_count(detail: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("/count".to_owned(), detail.to_owned())])
    }

    #[test]
    fn its_text_is_parsed() {
        assert_eq!(u16::from_part(part(b"42")).expect("a u16"), 42);
    }

    #[test]
    fn text_it_refuses_names_the_part_and_the_refusal() {
        let error = "x".parse::<u16>().expect_err("not a u16").to_string();
        assert_eq!(failures(u16::from_part(part(b"x"))), at_count(&error));
    }

    #[test]
    fn octets_that_are_no_text_name_the_part() {
        let expected = at_count("the part is not valid UTF-8");
        assert_eq!(failures(u16::from_part(part(b"\xff"))), expected);
        assert_eq!(failures(String::from_part(part(b"\xff"))), expected);
    }
}

/// The `boundary` is read by the same rule that accepted the media type:
/// RFC 9110 section 5.6.6 allows optional whitespace on both sides of each
/// `;`, so a `Content-Type` spelled with it decodes the parts rather than being
/// the 415 a stricter parser makes of it.
#[cfg(feature = "multipart")]
mod a_multipart_boundary_is_read_past_optional_whitespace {
    use crate::{
        error::rejection::BodyRejection,
        extract::body::multipart::{FromMultipart, MultipartForm, Part},
        http::{HeaderValue, Request, StatusCode, body::Body, header},
        schema::{Schema, registry::Registry},
    };

    /// Every part the body carried, as `(name, bytes)`.
    #[derive(Debug, PartialEq, Eq)]
    struct Parts(Vec<(String, bytes::Bytes)>);

    impl FromMultipart for Parts {
        fn from_parts(parts: Vec<Part>) -> Result<Self, BodyRejection> {
            Ok(Self(
                parts
                    .into_iter()
                    .map(|part| (part.name, part.bytes))
                    .collect(),
            ))
        }
    }

    impl Schema for Parts {
        fn schema(_registry: &mut Registry) -> kynos_openapi::Schema {
            kynos_openapi::Schema::any()
        }
    }

    /// One part, `a`, holding `b`, delimited by `x`.
    const BODY: &[u8] = b"--x\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nb\r\n--x--\r\n";

    /// What `MultipartForm` makes of [`BODY`] under `content_type`.
    async fn read(content_type: &'static str) -> Result<Parts, BodyRejection> {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(BODY)));
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        super::extract::<MultipartForm<Parts>>(request)
            .await
            .map(|MultipartForm(parts)| parts)
    }

    /// Whitespace before and after the delimiter, SP and HTAB, a trailing one,
    /// a quoted value, a preceding parameter, an empty trailing parameter, and
    /// names in another case.
    const SPELLINGS: &[&str] = &[
        "multipart/form-data;boundary=x",
        "multipart/form-data ; boundary=x",
        "multipart/form-data\t; boundary=x",
        "multipart/form-data;\tboundary=x",
        "multipart/form-data; boundary=x ",
        "multipart/form-data; boundary=x\t",
        "multipart/form-data \t;\t boundary=\"x\" ",
        "multipart/form-data; charset=utf-8 ; boundary=x",
        "multipart/form-data; boundary=x ;",
        "Multipart/Form-Data; Boundary=x",
    ];

    #[tokio::test]
    async fn every_spelling_decodes_the_parts() {
        for content_type in SPELLINGS {
            let parts = read(content_type)
                .await
                .unwrap_or_else(|rejection| panic!("`{content_type}`: {rejection:?}"));
            assert_eq!(
                parts,
                Parts(vec![("a".to_owned(), bytes::Bytes::from_static(b"b"))]),
                "`{content_type}`"
            );
        }
    }

    /// A `;` inside a quoted `boundary` is part of the delimiter, not the end of
    /// the parameter (RFC 9110 section 5.6.6), alone or after another quoted
    /// parameter.
    #[tokio::test]
    async fn a_quoted_semicolon_is_part_of_the_value() {
        for content_type in [
            "multipart/form-data; boundary=\"x;y\"",
            "multipart/form-data; charset=\"utf-8\"; boundary=\"x;y\" ",
        ] {
            let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(
                b"--x;y\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nb\r\n--x;y--\r\n",
            )));
            request
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            let MultipartForm(parts) = super::extract::<MultipartForm<Parts>>(request)
                .await
                .unwrap_or_else(|rejection| panic!("`{content_type}`: {rejection:?}"));
            assert_eq!(
                parts,
                Parts(vec![("a".to_owned(), bytes::Bytes::from_static(b"b"))]),
                "`{content_type}`"
            );
        }
    }

    /// A stream yielding each of its chunks in turn, then ending, and pending
    /// once before each, as a transport waiting on the next frame does; a
    /// stream always ready would be read whole before any part is parsed.
    struct Chunks {
        chunks: std::collections::VecDeque<bytes::Bytes>,
        ready: bool,
    }

    impl futures_core::Stream for Chunks {
        type Item = Result<bytes::Bytes, std::convert::Infallible>;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            if !std::mem::replace(&mut self.ready, false) {
                self.ready = true;
                context.waker().wake_by_ref();
                return std::task::Poll::Pending;
            }
            std::task::Poll::Ready(self.chunks.pop_front().map(Ok))
        }
    }

    /// A part's octets arrive whole however the transport splits them: here
    /// one octet per frame, so every non-empty part spans several chunks, and
    /// an empty part between two others stays empty.
    #[tokio::test]
    async fn a_part_split_across_frames_is_read_whole() {
        let body: &[u8] =
            b"--x\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nhello world\r\n\
            --x\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\n\r\n\
            --x\r\nContent-Disposition: form-data; name=\"c\"\r\n\r\nz\r\n--x--\r\n";
        let chunks = body
            .iter()
            .map(|octet| bytes::Bytes::copy_from_slice(std::slice::from_ref(octet)))
            .collect();
        let mut request = Request::new(Body::from_stream(Chunks {
            chunks,
            ready: false,
        }));
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );

        let MultipartForm(parts) = super::extract::<MultipartForm<Parts>>(request)
            .await
            .expect("a well-formed body");

        assert_eq!(
            parts,
            Parts(vec![
                ("a".to_owned(), bytes::Bytes::from_static(b"hello world")),
                ("b".to_owned(), bytes::Bytes::new()),
                ("c".to_owned(), bytes::Bytes::from_static(b"z")),
            ])
        );
    }

    /// An empty `boundary` delimits nothing, so it is the 400 a missing one is,
    /// not a media type the codec refuses.
    #[tokio::test]
    async fn an_empty_boundary_is_no_boundary() {
        for content_type in [
            "multipart/form-data",
            "multipart/form-data; boundary=",
            "multipart/form-data; boundary=\"\" ",
        ] {
            let rejection = read(content_type).await.expect_err(content_type);
            assert_eq!(
                rejection.status(),
                StatusCode::BAD_REQUEST,
                "`{content_type}`"
            );
            assert!(
                matches!(rejection, BodyRejection::Syntax { .. }),
                "`{content_type}`: {rejection:?}"
            );
        }
    }
}

/// A part's field name is its `Content-Disposition`'s `name` parameter, read
/// by RFC 9110's `quoted-string` grammar (section 5.6.4) under RFC 7578
/// section 4.2, which makes every part's disposition `form-data`.
#[cfg(feature = "multipart")]
mod a_part_disposition_is_read_quote_aware {
    use crate::{
        error::rejection::BodyRejection,
        extract::body::multipart::{
            FromMultipart, MultipartForm, Part,
            disposition::{self, Disposition},
        },
        http::{HeaderValue, Request, StatusCode, body::Body, header},
        schema::{Schema, registry::Registry},
    };

    /// Every part the body carried, as `(name, file_name)`.
    #[derive(Debug, PartialEq, Eq)]
    struct Names(Vec<(String, Option<String>)>);

    impl FromMultipart for Names {
        fn from_parts(parts: Vec<Part>) -> Result<Self, BodyRejection> {
            Ok(Self(
                parts
                    .into_iter()
                    .map(|part| (part.name, part.file_name))
                    .collect(),
            ))
        }
    }

    impl Schema for Names {
        fn schema(_registry: &mut Registry) -> kynos_openapi::Schema {
            kynos_openapi::Schema::any()
        }
    }

    /// What `MultipartForm` makes of one part carrying `disposition`.
    async fn read(disposition: &str) -> Result<Names, BodyRejection> {
        read_head(format!("Content-Disposition: {disposition}").as_bytes()).await
    }

    /// What `MultipartForm` makes of one part whose header block is `head`.
    async fn read_head(head: &[u8]) -> Result<Names, BodyRejection> {
        let body = [b"--x\r\n", head, b"\r\n\r\nb\r\n--x--\r\n"].concat();
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from(body)));
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );
        super::extract::<MultipartForm<Names>>(request)
            .await
            .map(|MultipartForm(names)| names)
    }

    /// A ` name=` inside a quoted `filename` is part of the file name, not
    /// the field's.
    #[tokio::test]
    async fn a_name_inside_a_quoted_value_is_not_the_name() {
        let names = read(r#"form-data; filename="; name=x"; name="real""#)
            .await
            .expect("a well-formed disposition");
        assert_eq!(
            names,
            Names(vec![("real".to_owned(), Some("; name=x".to_owned()))])
        );
    }

    /// A disposition other than `form-data` is not a form field: 400.
    #[tokio::test]
    async fn a_disposition_other_than_form_data_is_refused() {
        for disposition in [
            r#"attachment; name="a""#,
            r#"inline; name="a""#,
            r#"form-datax; name="a""#,
            r#"name="a""#,
        ] {
            let rejection = read(disposition).await.expect_err(disposition);
            assert_eq!(
                rejection.status(),
                StatusCode::BAD_REQUEST,
                "`{disposition}`"
            );
            assert!(
                matches!(rejection, BodyRejection::Syntax { .. }),
                "`{disposition}`: {rejection:?}"
            );
        }
    }

    /// The detail of the 400 `head` earns, which must be a syntax rejection.
    async fn refused(head: &[u8]) -> String {
        match read_head(head).await {
            Err(BodyRejection::Syntax { detail }) => detail,
            other => panic!("{:?}: {other:?}", String::from_utf8_lossy(head)),
        }
    }

    /// A part with no `Content-Disposition` names no field: 400.
    #[tokio::test]
    async fn a_part_without_a_disposition_is_refused() {
        assert_eq!(
            refused(b"Content-Type: text/plain").await,
            "a part carries no `Content-Disposition`"
        );
    }

    /// A part whose `form-data` disposition names no field is 400, file name
    /// or not: RFC 7578 section 4.2 requires each part to carry a `name`.
    #[tokio::test]
    async fn a_part_naming_no_field_is_refused() {
        for head in [
            &b"Content-Disposition: form-data"[..],
            b"Content-Disposition: form-data; filename=\"a.txt\"",
        ] {
            assert_eq!(
                refused(head).await,
                "a part declares no field name in its `Content-Disposition`"
            );
        }
    }

    /// A `Content-Disposition` that is not UTF-8 is 400, the file name's
    /// bytes included, since both names are `String`s.
    #[tokio::test]
    async fn a_disposition_that_is_not_utf8_is_refused() {
        for head in [
            &b"Content-Disposition: form-data; name=\"\xff\""[..],
            b"Content-Disposition: form-data; name=\"a\"; filename=\"\xff.txt\"",
        ] {
            assert_eq!(
                refused(head).await,
                "a part's `Content-Disposition` is not UTF-8"
            );
        }
    }

    /// The field a well-formed disposition names, as `(name, file_name)`.
    fn named(value: &str) -> (String, Option<String>) {
        let Disposition { name, file_name } =
            disposition::parse(value).unwrap_or_else(|detail| panic!("`{value}`: {detail}"));
        (name, file_name)
    }

    /// Each spelling the grammar admits names field `a`, with the file name
    /// it carries.
    #[test]
    fn each_admitted_spelling_is_read() {
        let file = |name: &str| Some(name.to_owned());
        for (value, file_name) in [
            // A token value, and the type and parameter names in any case.
            ("form-data; name=a", None),
            ("Form-Data; NAME=\"a\"", None),
            // Whitespace around `;` and `=`, and empty parameters.
            ("form-data ;\tname = \"a\" ;", None),
            ("form-data;;name=\"a\";;", None),
            // Separators, quotes and backslashes inside a quoted value.
            (r#"form-data; name="a"; filename="x;y=z""#, file("x;y=z")),
            (
                r#"form-data; filename="\"; name=b"; name="a""#,
                file("\"; name=b"),
            ),
            (
                r#"form-data; name="a"; filename="c:\\x\.txt""#,
                file(r"c:\x.txt"),
            ),
            // Raw UTF-8 is `obs-text`; a percent escape is kept as sent.
            ("form-data; name=\"a\"; filename=\"é.txt\"", file("é.txt")),
            (
                "form-data; name=\"a\"; filename=\"%22.txt\"",
                file("%22.txt"),
            ),
            // An unknown parameter, `filename*` included, is read past.
            (
                "form-data; filename*=UTF-8''x.txt; name=\"a\"; size=3",
                None,
            ),
        ] {
            assert_eq!(named(value), ("a".to_owned(), file_name), "`{value}`");
        }
    }

    /// An empty quoted value is a value: the field is named by the empty
    /// string, which `FromMultipart` then matches against no declared field.
    #[test]
    fn an_empty_quoted_name_is_a_name() {
        assert_eq!(named("form-data; name=\"\""), (String::new(), None));
    }

    /// Each value the grammar refuses, or whose field it cannot name.
    #[test]
    fn each_refused_spelling_names_no_field() {
        for value in [
            "",
            "form-data",
            "form-data; filename=\"x\"",
            // Two of a parameter the codec reads.
            "form-data; name=a; NAME=b",
            "form-data; name=a; filename=x; filename=y",
            // Unterminated, or holding a control character.
            "form-data; name=\"a",
            "form-data; name=\"a\\",
            "form-data; name=\"a\u{1}\"",
            "form-data; name=\"a\\\u{7f}\"",
            // Not a token, missing `=` or value, or nothing between values.
            "form-data; name=a/b",
            "form-data; name=",
            "form-data; name",
            "form-data; =a",
            "form-data; name=\"a\" x",
            "form-data name=a",
        ] {
            assert!(disposition::parse(value).is_err(), "`{value}` was admitted");
        }
    }
}

/// A form body decodes the encoding its description states.
///
/// No Encoding Object is written, so each property takes the defaults:
/// `style: form` with `explode: true`, under which an array property is one
/// pair per item, all under the property's name. A field the description calls
/// an array therefore has to accept a repeated key, and a single pair as a
/// one-item array.
#[cfg(feature = "form")]
mod a_form_body_decodes_what_it_describes {
    use std::collections::BTreeMap;

    use crate::{
        error::rejection::BodyRejection,
        extract::body::form::Form,
        http::{HeaderValue, Request, StatusCode, body::Body, header},
        schema::{Schema, registry::Registry},
    };

    /// Gives each type an open schema declaring no bound: these tests read
    /// the decoding, which no bound takes part in.
    macro_rules! unbounded {
        ($($ty:ty),+ $(,)?) => {$(
            impl Schema for $ty {
                fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
                    let _ = registry;
                    kynos_openapi::Schema::any()
                }
            }
        )+};
    }

    /// Reads `T` from a form body holding `body`.
    async fn read<T: serde::de::DeserializeOwned + Schema + Send>(
        body: &'static [u8],
    ) -> Result<T, BodyRejection> {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(body)));
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        super::extract::<Form<T>>(request)
            .await
            .map(|Form(value)| value)
    }

    #[derive(Debug, serde::Deserialize)]
    struct Tags {
        tag: Vec<String>,
    }
    unbounded!(Tags);

    #[tokio::test]
    async fn a_repeated_key_is_one_item_per_pair_in_order() {
        let tags = read::<Tags>(b"tag=a&tag=b").await.expect("an array");

        assert_eq!(tags.tag, ["a", "b"]);
    }

    #[tokio::test]
    async fn a_single_pair_is_a_one_item_array() {
        let tags = read::<Tags>(b"tag=a").await.expect("an array");

        assert_eq!(tags.tag, ["a"]);
    }

    /// A form that does not fit `T` is one failure keyed at the root, whose
    /// detail is the decoder's own message, unchanged.
    #[tokio::test]
    async fn a_misfit_form_is_the_decoders_message_at_the_root() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Page {
            page: u32,
        }
        unbounded!(Page);

        let decoder = serde_html_form::from_str::<Page>("page=first")
            .expect_err("`first` is not a `u32`")
            .to_string();
        let rejection = read::<Page>(b"page=first")
            .await
            .expect_err("`first` is not a `u32`");

        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let BodyRejection::Schema { failures } = rejection else {
            panic!("expected a schema failure, got {rejection:?}");
        };
        assert_eq!(
            failures.into_iter().collect::<Vec<_>>(),
            [(String::new(), decoder)]
        );
    }

    /// The items are parsed as the item type, not only collected as text.
    #[tokio::test]
    async fn the_items_are_read_as_the_item_type() {
        #[derive(Debug, serde::Deserialize)]
        struct Ids {
            id: Vec<u32>,
        }
        unbounded!(Ids);

        let ids = read::<Ids>(b"id=3&other=x&id=5").await.expect("an array");

        assert_eq!(ids.id, [3, 5]);
    }

    /// A scalar property is one pair. A second one does not fit the schema,
    /// so it is refused as a 422 rather than resolved by picking one of the
    /// two values.
    #[tokio::test]
    async fn a_repeated_scalar_key_is_422() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Page {
            page: u32,
        }
        unbounded!(Page);

        let rejection = read::<Page>(b"page=1&page=2")
            .await
            .expect_err("a scalar takes one pair");

        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let BodyRejection::Schema { failures } = rejection else {
            panic!("expected a schema failure, got {rejection:?}");
        };
        assert_eq!(failures.keys().collect::<Vec<_>>(), [""]);
    }

    /// A map of scalars is held to the same rule as a struct of them.
    #[tokio::test]
    async fn a_repeated_key_into_a_map_of_scalars_is_422() {
        let rejection = read::<BTreeMap<String, String>>(b"a=1&a=2")
            .await
            .expect_err("a scalar value takes one pair");

        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// An empty number input is submitted as `name=`, which is no number; an
    /// optional field reads it as absent rather than refusing the form.
    #[tokio::test]
    async fn an_empty_optional_number_is_none() {
        #[derive(Debug, serde::Deserialize)]
        struct Filter {
            limit: Option<u32>,
        }
        unbounded!(Filter);

        let filter = read::<Filter>(b"limit=").await.expect("an absent number");

        assert_eq!(filter.limit, None);
    }

    /// The two shapes `Form`'s documentation says are described but not
    /// decoded, held to the 422 it promises for them.
    #[tokio::test]
    async fn a_nested_struct_and_a_flattened_number_are_422() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Inner {
            n: u32,
        }
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Nested {
            inner: Inner,
        }
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Flattened {
            #[serde(flatten)]
            inner: Inner,
        }
        unbounded!(Nested, Flattened);

        let nested = read::<Nested>(b"n=1").await.expect_err("not decoded");
        let flattened = read::<Flattened>(b"n=1").await.expect_err("not decoded");

        assert_eq!(nested.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(flattened.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

/// The message a protobuf body carries is the one the extractor yields.
///
/// A non-default value, so an extractor that skipped the decode and yielded
/// `T::default()` cannot pass.
#[cfg(feature = "protobuf")]
#[tokio::test]
async fn a_protobuf_body_decodes_to_the_message_it_encodes() {
    use crate::{
        extract::body::protobuf::Protobuf,
        http::{HeaderValue, body::Body, header},
    };

    #[derive(Clone, PartialEq, prost::Message)]
    struct Tagged {
        #[prost(int32, tag = "1")]
        value: i32,
    }

    // An open schema declaring no bound: this reads the decoding alone.
    impl crate::schema::Schema for Tagged {
        fn schema(registry: &mut crate::schema::registry::Registry) -> kynos_openapi::Schema {
            let _ = registry;
            kynos_openapi::Schema::any()
        }
    }

    // Field 1, varint wire type, value 7 -- the whole message.
    let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(&[0x08, 0x07])));
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/protobuf"),
    );

    let Protobuf(message) = extract::<Protobuf<Tagged>>(request)
        .await
        .expect("a well-formed message");

    assert_eq!(message, Tagged { value: 7 });
}

/// Bytes protobuf cannot decode are a 400 syntax rejection, not a 422: no
/// message exists for a bound to be checked against. The detail is `prost`'s
/// own sentence, so only its presence is asserted.
#[cfg(feature = "protobuf")]
#[tokio::test]
async fn a_protobuf_body_that_does_not_decode_is_a_syntax_rejection() {
    use crate::{
        error::rejection::BodyRejection,
        extract::body::protobuf::Protobuf,
        http::{HeaderValue, StatusCode, body::Body, header},
    };

    // Field 1, varint wire type, and then no varint: the buffer ends first.
    let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(&[0x08])));
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/protobuf"),
    );

    let rejection = extract::<Protobuf<()>>(request)
        .await
        .expect_err("a truncated message");

    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
    let BodyRejection::Syntax { detail } = rejection else {
        panic!("refused as {rejection:?} rather than as syntax");
    };
    assert!(!detail.is_empty());
}

/// A body still under a content coding was decoded by nothing, since
/// `Decompression` removes the field once it decodes: every codec refuses it
/// with 415 rather than reading the coded bytes as its media type (RFC 9110
/// sections 8.4 and 15.5.16), and `identity`, which is no coding, still reads.
mod an_undecoded_content_coding_is_refused {
    use crate::{
        error::rejection::BodyRejection,
        extract::{
            FromRequest,
            body::{binary::Binary, text::Text},
        },
        http::{HeaderValue, Request, StatusCode, body::Body, header, media::OctetStream},
    };

    /// A `content_type` request carrying `body` under each of `codings`, one
    /// `Content-Encoding` field per entry.
    fn coded(
        content_type: &'static str,
        codings: &[&'static [u8]],
        body: &'static [u8],
    ) -> Request {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(body)));
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        for coding in codings {
            request.headers_mut().append(
                header::CONTENT_ENCODING,
                HeaderValue::from_bytes(coding).expect("a field value"),
            );
        }
        request
    }

    /// What `T` refuses `coded(..)` with, or `None` where it reads it.
    async fn refusal<T: FromRequest<(), Rejection = BodyRejection>>(
        content_type: &'static str,
        codings: &[&'static [u8]],
        body: &'static [u8],
    ) -> Option<BodyRejection> {
        super::extract::<T>(coded(content_type, codings, body))
            .await
            .err()
    }

    /// The field values that leave a coding undecoded: each coding Kynos can
    /// decode, one it cannot, one listed after `identity`, one in a second
    /// field, and a value that is not text.
    const UNDECODED: &[&[&[u8]]] = &[
        &[b"gzip"],
        &[b"br"],
        &[b"zstd"],
        &[b"x-unknown"],
        &[b"identity, gzip"],
        &[b"identity", b"br"],
        &[b"\xff"],
    ];

    /// The field values that declare no coding at all.
    const IDENTITY: &[&[&[u8]]] = &[&[b"identity"], &[b"IDENTITY, identity"], &[b""]];

    /// Asserts that `T` refuses every [`UNDECODED`] value with 415 and reads
    /// every [`IDENTITY`] one.
    async fn holds<T: FromRequest<(), Rejection = BodyRejection>>(
        content_type: &'static str,
        body: &'static [u8],
    ) {
        for codings in UNDECODED {
            let rejection = refusal::<T>(content_type, codings, body).await;
            assert_eq!(
                rejection.as_ref().map(BodyRejection::status),
                Some(StatusCode::UNSUPPORTED_MEDIA_TYPE),
                "`{content_type}` under {codings:?}: {rejection:?}"
            );
            assert!(
                matches!(rejection, Some(BodyRejection::UnsupportedCoding { .. })),
                "a coding refusal, not a media type one: {rejection:?}"
            );
        }
        for codings in IDENTITY {
            let rejection = refusal::<T>(content_type, codings, body).await;
            assert!(
                rejection.is_none(),
                "`{content_type}` under {codings:?}: {rejection:?}"
            );
        }
    }

    /// The refusal quotes every field the client sent, in order, a value that
    /// is not text lossily.
    #[tokio::test]
    async fn the_refusal_quotes_what_was_sent() {
        let received = |codings| async move {
            match refusal::<Text>("text/plain", codings, b"").await {
                Some(BodyRejection::UnsupportedCoding { received }) => received,
                other => panic!("expected a coding refusal, got {other:?}"),
            }
        };

        assert_eq!(received(&[b"identity", b"br"]).await, "identity, br");
        assert_eq!(received(&[b"gzip, br"]).await, "gzip, br");
        assert_eq!(received(&[b"\xff"]).await, "\u{fffd}");
    }

    /// A body in the wrong media type is refused for that first, as it is
    /// without a coding.
    #[tokio::test]
    async fn the_media_type_is_enforced_first() {
        let rejection = refusal::<Text>("application/json", &[b"gzip"], b"").await;

        assert!(
            matches!(
                rejection,
                Some(BodyRejection::UnsupportedMediaType { received: Some(ref received) })
                    if received == "application/json"
            ),
            "{rejection:?}"
        );
    }

    /// The same order holds for a streamed body, which enforces both 415s
    /// before it takes the body rather than before it buffers one.
    #[cfg(all(feature = "json", feature = "openapi32"))]
    #[tokio::test]
    async fn the_media_type_is_enforced_first_by_a_streamed_body() {
        use crate::extract::body::json_lines::{JsonLines, JsonSeq, records::Records};

        for rejection in [
            refusal::<JsonLines<Records<u32>>>("text/plain", &[b"gzip"], b"1\n").await,
            refusal::<JsonSeq<Records<u32>>>("text/plain", &[b"gzip"], b"\x1e1\n").await,
        ] {
            assert!(
                matches!(
                    rejection,
                    Some(BodyRejection::UnsupportedMediaType { received: Some(ref received) })
                        if received == "text/plain"
                ),
                "{rejection:?}"
            );
        }
    }

    #[tokio::test]
    async fn by_a_binary_body() {
        holds::<Binary<OctetStream>>("application/octet-stream", b"\x1f\x8b").await;
    }

    #[tokio::test]
    async fn by_a_text_body() {
        holds::<Text>("text/plain", b"text").await;
    }

    #[cfg(feature = "json")]
    #[tokio::test]
    async fn by_a_json_body() {
        holds::<crate::extract::body::json::Json<u32>>("application/json", b"1").await;
    }

    #[cfg(feature = "form")]
    #[tokio::test]
    async fn by_a_form_body() {
        holds::<crate::extract::body::form::Form<std::collections::BTreeMap<String, String>>>(
            "application/x-www-form-urlencoded",
            b"a=b",
        )
        .await;
    }

    #[cfg(feature = "protobuf")]
    #[tokio::test]
    async fn by_a_protobuf_body() {
        holds::<crate::extract::body::protobuf::Protobuf<()>>("application/protobuf", b"").await;
    }

    #[cfg(all(feature = "json", feature = "openapi32"))]
    #[tokio::test]
    async fn by_a_streamed_json_body() {
        use crate::extract::body::json_lines::{JsonLines, JsonSeq, records::Records};

        holds::<JsonLines<Records<u32>>>("application/x-ndjson", b"1\n").await;
        holds::<JsonSeq<Records<u32>>>("application/json-seq", b"\x1e1\n").await;
    }

    #[cfg(feature = "multipart")]
    #[tokio::test]
    async fn by_a_multipart_body() {
        use crate::{
            extract::body::multipart::{FromMultipart, MultipartForm, Part},
            schema::{Schema, registry::Registry},
        };

        /// Built from any parts, so only the coding decides.
        struct Anything;

        impl FromMultipart for Anything {
            fn from_parts(_parts: Vec<Part>) -> Result<Self, BodyRejection> {
                Ok(Self)
            }
        }

        impl Schema for Anything {
            fn schema(_registry: &mut Registry) -> kynos_openapi::Schema {
                kynos_openapi::Schema::any()
            }
        }

        holds::<MultipartForm<Anything>>(
            "multipart/form-data; boundary=x",
            b"--x\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nb\r\n--x--\r\n",
        )
        .await;
    }
}

/// `Content-Type` is a singleton field, and a recipient that meets it twice
/// may take either line (RFC 9110 section 8.3), so an intermediary and Kynos
/// could read one body as two types: every body refuses the request with 415,
/// even where both lines agree, and quotes every line it was sent.
mod two_content_type_lines_are_refused {
    use crate::{
        error::rejection::BodyRejection,
        extract::{
            FromRequest,
            body::{OneOf, binary::Binary, offers, text::Text},
        },
        http::{
            HeaderMap, HeaderValue, Request, StatusCode, body::Body, header, media::OctetStream,
        },
    };

    /// Headers carrying one `Content-Type` field line per entry of `lines`.
    fn headers(lines: &[&'static str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for line in lines {
            headers.append(header::CONTENT_TYPE, HeaderValue::from_static(line));
        }
        headers
    }

    /// What `T` refuses an empty body under `lines` with, or `None` where it
    /// reads it.
    async fn refusal<T: FromRequest<(), Rejection = BodyRejection>>(
        lines: &[&'static str],
    ) -> Option<BodyRejection> {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::new()));
        *request.headers_mut() = headers(lines);
        super::extract::<T>(request).await.err()
    }

    /// The pairs refused: the same line twice, and two different types the
    /// body reads either of.
    const TWICE: &[&[&str]] = &[
        &["text/plain", "text/plain"],
        &["text/plain", "application/octet-stream"],
        &["application/octet-stream", "text/plain"],
    ];

    #[test]
    fn no_media_type_is_offered() {
        for lines in TWICE {
            let headers = headers(lines);
            for media_type in *lines {
                assert!(
                    !offers(&headers, media_type),
                    "{lines:?} offers `{media_type}`"
                );
            }
        }
        assert!(offers(&headers(&["text/plain"]), "text/plain"));
    }

    /// Asserts that `T` answers every [`TWICE`] pair with a 415 quoting both
    /// lines in order.
    async fn holds<T: FromRequest<(), Rejection = BodyRejection>>() {
        for lines in TWICE {
            let rejection = refusal::<T>(lines).await;
            assert_eq!(
                rejection.as_ref().map(BodyRejection::status),
                Some(StatusCode::UNSUPPORTED_MEDIA_TYPE),
                "{lines:?}: {rejection:?}"
            );
            let expected = lines.join(", ");
            assert!(
                matches!(
                    rejection,
                    Some(BodyRejection::UnsupportedMediaType { received: Some(ref received) })
                        if *received == expected
                ),
                "{lines:?}: {rejection:?}"
            );
        }
    }

    #[tokio::test]
    async fn by_a_text_body() {
        holds::<Text>().await;
    }

    #[tokio::test]
    async fn by_a_binary_body() {
        holds::<Binary<OctetStream>>().await;
    }

    #[tokio::test]
    async fn by_either_side_of_a_one_of() {
        holds::<OneOf<Text, Binary<OctetStream>>>().await;
    }

    /// One line still reads, so the refusal is the count and nothing else.
    #[tokio::test]
    async fn one_line_is_read() {
        assert!(refusal::<Text>(&["text/plain"]).await.is_none());
        assert!(
            refusal::<Binary<OctetStream>>(&["application/octet-stream"])
                .await
                .is_none()
        );
    }
}

/// A `Content-Type` carrying obs-text is still a declared one: the 415 quotes
/// it lossily rather than telling the client it declared none.
#[test]
fn an_unreadable_content_type_is_quoted_lossily() {
    use crate::{
        error::problem::IntoProblem,
        extract::body::unsupported_media_type,
        http::{HeaderMap, HeaderValue, header},
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_bytes(b"text/plain; x=\xff").expect("obs-text is a valid field value"),
    );

    let problem = serde_json::to_value(unsupported_media_type(&headers).into_problem())
        .expect("a problem serializes");

    assert_eq!(
        problem["detail"],
        "unsupported media type: `text/plain; x=\u{fffd}`"
    );
}

/// An optional body without a `Content-Type` is decided by whether its content
/// carries an octet, on a body the transport has not ended as on the ended
/// `Body::from_bytes` a buffering interceptor hands on, which `extractors.rs`
/// drives.
mod an_optional_body_without_a_content_type {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    use bytes::Bytes;

    use crate::{
        error::rejection::BodyRejection,
        extract::body::text::Text,
        http::{Request, body::Body},
    };

    /// Chunks yielded in turn, one frame each, as HTTP/2 DATA frames arrive.
    struct Frames(VecDeque<Bytes>);

    impl futures_core::Stream for Frames {
        type Item = Result<Bytes, Infallible>;

        fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.get_mut().0.pop_front().map(Ok))
        }
    }

    /// A body the transport has not ended yet, yielding `chunks`.
    fn streamed(chunks: &[&'static [u8]]) -> Body {
        let body = Body::from_stream(Frames(
            chunks.iter().copied().map(Bytes::from_static).collect(),
        ));
        assert!(
            !http_body::Body::is_end_stream(&body),
            "the body has not stated its end before it is read"
        );
        body
    }

    async fn read(body: Body) -> Result<Option<Text>, BodyRejection> {
        super::extract::<Option<Text>>(Request::new(body)).await
    }

    /// Empty DATA frames are zero-length content, absent as the empty
    /// `Body::from_bytes` that `BodySize` and `Decompression` rebuild them as.
    #[tokio::test]
    async fn empty_data_frames_are_absent() {
        assert_eq!(read(streamed(&[b""])).await.expect("absent"), None);
        assert_eq!(read(streamed(&[b"", b""])).await.expect("absent"), None);
    }

    /// An octet after empty frames is content: the 415 naming no type.
    #[tokio::test]
    async fn an_octet_after_empty_frames_is_415() {
        let rejection = read(streamed(&[b"", b"x"]))
            .await
            .expect_err("content without a type");
        assert!(
            matches!(
                rejection,
                BodyRejection::UnsupportedMediaType { received: None }
            ),
            "{rejection:?}"
        );
    }

    /// A body failing before its first octet is the 400 a codec's read
    /// answers, not a body known to be absent.
    #[tokio::test]
    async fn a_body_failing_before_its_first_octet_is_400() {
        let rejection = read(Body::failed_after(Bytes::new(), "reset".into()))
            .await
            .expect_err("an unfinished body");
        assert!(
            matches!(rejection, BodyRejection::Syntax { .. }),
            "{rejection:?}"
        );
    }

    /// What an empty body declaring `values`, one `Content-Length` line each,
    /// is read as.
    async fn declared(values: &[&'static [u8]]) -> Result<Option<Text>, BodyRejection> {
        let mut request = Request::new(Body::empty());
        for value in values {
            request.headers_mut().append(
                crate::http::header::CONTENT_LENGTH,
                crate::http::HeaderValue::from_bytes(value).expect("a field value"),
            );
        }
        super::extract::<Option<Text>>(request).await
    }

    /// A declared zero is read past surrounding whitespace, on every line; any
    /// line that is not a decimal zero declares content, the 415 naming no
    /// type, before the body is read at all. `+0` is a zero as `u64`'s parser
    /// reads it, which is more lenient than RFC 9110 section 8.6's `1*DIGIT`
    /// and pinned as it stands, for the reason `limit`'s case gives.
    #[tokio::test]
    async fn a_declared_zero_is_read_past_whitespace_on_every_line() {
        for values in [
            &[&b" 0 "[..]][..],
            &[&b"\t0"[..]][..],
            &[&b"+0"[..]][..],
            &[&b"0"[..], &b" 0"[..]][..],
        ] {
            assert_eq!(declared(values).await.expect("absent"), None, "{values:?}");
        }

        for values in [
            &[&b""[..]][..],
            &[&b"-0"[..]][..],
            &[&b"0x0"[..]][..],
            &[&b"0 0"[..]][..],
            &[&b"\xff"[..]][..],
            &[&b" 0"[..], &b"1"[..]][..],
        ] {
            let rejection = declared(values).await.expect_err("declared content");
            assert!(
                matches!(
                    rejection,
                    BodyRejection::UnsupportedMediaType { received: None }
                ),
                "{values:?}: {rejection:?}"
            );
        }
    }
}

/// `limit::read`, the read under every buffering codec: a lone data frame is
/// handed over as it arrived, several are joined, and the limit holds over
/// their running total either way.
mod a_body_is_read_whole_under_its_limit {
    use std::{
        collections::VecDeque,
        pin::Pin,
        task::{Context, Poll},
    };

    use bytes::Bytes;
    use http_body::Frame;

    use crate::{
        error::rejection::BodyRejection,
        extract::body::limit,
        http::{
            HeaderMap,
            body::{Body, BoxError},
        },
    };

    /// Frames yielded in turn, as a transport delivers them.
    struct Frames(VecDeque<Result<Frame<Bytes>, BoxError>>);

    impl http_body::Body for Frames {
        type Data = Bytes;
        type Error = BoxError;

        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
            Poll::Ready(self.get_mut().0.pop_front())
        }
    }

    /// A data frame that arrived, as an item of a sequence that may also fail.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the item type of a frame sequence that may also fail"
    )]
    fn data(octets: &'static [u8]) -> Result<Frame<Bytes>, BoxError> {
        Ok(Frame::data(Bytes::from_static(octets)))
    }

    fn body(frames: impl IntoIterator<Item = Result<Frame<Bytes>, BoxError>>) -> Body {
        Body::from_body(Frames(frames.into_iter().collect()))
    }

    /// The one-frame body is the frame itself, same octets at the same
    /// address: neither copied nor queued behind a collector.
    #[tokio::test]
    async fn a_lone_frame_is_handed_over_uncopied() {
        let frame = Bytes::from_static(b"one frame");
        let address = frame.as_ptr();

        let read = limit::read(Body::from_bytes(frame), 64)
            .await
            .expect("within the limit");

        assert_eq!(read, b"one frame".as_slice());
        assert_eq!(read.as_ptr(), address, "the frame was copied");
    }

    /// Empty data frames and trailers around a lone data frame leave it a
    /// lone frame, so it is still handed over uncopied.
    #[tokio::test]
    async fn empty_frames_and_trailers_do_not_join_a_lone_frame() {
        static OCTETS: &[u8] = b"payload";
        let read = limit::read(
            body([
                data(b""),
                data(OCTETS),
                data(b""),
                Ok(Frame::trailers(HeaderMap::new())),
            ]),
            64,
        )
        .await
        .expect("within the limit");

        assert_eq!(read, OCTETS);
        assert_eq!(read.as_ptr(), OCTETS.as_ptr(), "the frame was copied");
    }

    /// Several data frames come back joined, in arrival order.
    #[tokio::test]
    async fn several_frames_are_joined_in_order() {
        let read = limit::read(body([data(b"ab"), data(b"cd"), data(b"e")]), 64)
            .await
            .expect("within the limit");

        assert_eq!(read, b"abcde".as_slice());
    }

    /// The limit is on the running total, inclusive: frames summing to it are
    /// read, and one octet more is the 413, whichever frame carries it.
    #[tokio::test]
    async fn the_limit_holds_over_the_running_total() {
        let at = limit::read(body([data(b"abc"), data(b"de")]), 5).await;
        assert_eq!(at.expect("exactly at the limit"), b"abcde".as_slice());

        let past = limit::read(body([data(b"abc"), data(b"def")]), 5)
            .await
            .expect_err("one octet past the limit");
        assert!(
            matches!(past, BodyRejection::TooLarge { limit: 5 }),
            "{past:?}"
        );

        let lone = limit::read(body([data(b"abcdef")]), 5)
            .await
            .expect_err("a lone frame past the limit");
        assert!(
            matches!(lone, BodyRejection::TooLarge { limit: 5 }),
            "{lone:?}"
        );
    }

    /// A transport failure after a frame arrived is the 400 quoting it, never
    /// the octets that arrived passed off as the whole body.
    #[tokio::test]
    async fn a_failure_part_way_is_400() {
        let rejection = limit::read(body([data(b"ab"), Err("reset".into())]), 64)
            .await
            .expect_err("an unfinished body");

        assert!(
            matches!(&rejection, BodyRejection::Syntax { detail } if detail == "reset"),
            "{rejection:?}"
        );
    }

    /// A head carrying one `Content-Length` line per entry of `values`.
    fn declaring(values: &[&'static [u8]]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                crate::http::header::CONTENT_LENGTH,
                crate::http::HeaderValue::from_bytes(value).expect("a field value"),
            );
        }
        headers
    }

    /// The length a head declares is its first `Content-Length` line, read
    /// past surrounding whitespace as an unsigned decimal; a line that is not
    /// one declares nothing, so the limit falls to the read.
    ///
    /// Two readings here are more lenient than RFC 9110 section 8.6's
    /// `1*DIGIT`, and are pinned as they stand so a change to either is made
    /// on purpose: a leading `+`, which `u64`'s parser accepts, and a second
    /// line, which is never read. HTTP/1.1 framing refuses both before a
    /// request reaches an extractor.
    #[test]
    fn the_declared_length_is_the_first_line_read_as_a_decimal() {
        for (values, expected) in [
            (&[][..], None),
            (&[&b"5"[..]][..], Some(5)),
            (&[&b" 5 "[..]][..], Some(5)),
            (&[&b"\t5\t"[..]][..], Some(5)),
            (&[&b"0"[..]][..], Some(0)),
            (&[&b"+5"[..]][..], Some(5)),
            (&[&b"5"[..], &b"7"[..]][..], Some(5)),
            (&[&b""[..]][..], None),
            (&[&b"abc"[..]][..], None),
            (&[&b"-1"[..]][..], None),
            (&[&b"0x5"[..]][..], None),
            (&[&b"5 5"[..]][..], None),
            (&[&b"5, 5"[..]][..], None),
            (&[&b"18446744073709551616"[..]][..], None),
            (&[&b"\xff5"[..]][..], None),
        ] {
            assert_eq!(
                limit::declared_length(&declaring(values)),
                expected,
                "{values:?}"
            );
        }
    }

    /// A declared length refuses from the head only past the limit, and one
    /// that declares nothing is left to the read.
    #[test]
    fn a_declared_length_is_refused_only_past_the_limit() {
        assert!(limit::refuse_declared(&declaring(&[b" 5 "]), 5).is_ok());
        assert!(limit::refuse_declared(&declaring(&[b"abc"]), 5).is_ok());
        assert!(limit::refuse_declared(&declaring(&[]), 5).is_ok());

        let refused = limit::refuse_declared(&declaring(&[b"\t6"]), 5)
            .expect_err("six octets declared past a five-octet limit");
        assert!(
            matches!(refused, BodyRejection::TooLarge { limit: 5 }),
            "{refused:?}"
        );
    }
}

/// The constant a `OneOf` holding a `JsonAs` evaluates refuses a marker
/// sharing requests with the body beside it, by the rule `Binary`'s is
/// held to above.
#[cfg(feature = "json")]
#[test]
#[should_panic(expected = "a `JsonAs` marker is offered requests the body beside it")]
fn a_shared_json_marker_is_refused_by_the_constant_one_of_evaluates() {
    crate::extract::body::alternative::distinct_json(
        "application/vnd.acme+json; version=2",
        "Application/VND.acme+json",
    );
}

/// The control: a version in the subtype is another media type.
#[cfg(feature = "json")]
#[test]
fn a_json_marker_of_another_subtype_is_admitted() {
    crate::extract::body::alternative::distinct_json(
        "application/vnd.acme.v2+json",
        "application/vnd.acme.v1+json",
    );
}

/// A JSON document under a declared media type reads and refuses as `Json`
/// does, keyed by its own media type rather than `application/json`.
#[cfg(feature = "json")]
mod a_json_body_under_a_declared_media_type {
    use kynos_openapi::RefOr;

    use crate::{
        error::rejection::BodyRejection,
        extract::{
            body::{json::Json, json_as::JsonAs},
            describe::{Describe, RequestContent},
        },
        http::{HeaderValue, Request, body::Body, header, media::MediaType},
        router::operation::OperationCx,
        schema::{
            Schema,
            constraints::{Pointer, Violations},
            registry::Registry,
        },
    };

    /// A vendor type carrying a parameter, which a request need not repeat.
    #[derive(Debug)]
    struct Order;

    impl MediaType for Order {
        const MEDIA_TYPE: &'static str = "application/vnd.acme.order+json; version=2";
    }

    /// A document whose schema declares a bound `serde` does not enforce.
    #[derive(Debug, serde::Deserialize)]
    struct Quantity {
        count: u32,
    }

    impl Schema for Quantity {
        fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
            registry.resolve::<u32>()
        }

        fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
            if self.count == 0 {
                violations.report(at.member("count"), "must be at least 1");
            }
        }
    }

    fn request(content_type: &'static str, body: &'static [u8]) -> Request {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(body)));
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        request
    }

    #[tokio::test]
    async fn reads_its_own_media_type_in_any_case_with_or_without_its_parameter() {
        for content_type in [
            "application/vnd.acme.order+json",
            "Application/VND.Acme.Order+JSON; version=2",
            "application/vnd.acme.order+json; charset=utf-8",
        ] {
            let order = super::extract::<JsonAs<Order, u32>>(request(content_type, b"7"))
                .await
                .unwrap_or_else(|rejection| panic!("{content_type}: {rejection:?}"));
            assert_eq!(order.into_inner(), 7);
        }
    }

    #[tokio::test]
    async fn application_json_is_another_media_type() {
        let rejection = super::extract::<JsonAs<Order, u32>>(request("application/json", b"7"))
            .await
            .expect_err("`application/json` is not the declared type");
        assert!(
            matches!(rejection, BodyRejection::UnsupportedMediaType { .. }),
            "{rejection:?}"
        );

        // And the reverse: `Json` reads no `+json` type.
        let rejection =
            super::extract::<Json<u32>>(request("application/vnd.acme.order+json", b"7"))
                .await
                .expect_err("a `+json` type is not `application/json`");
        assert!(
            matches!(rejection, BodyRejection::UnsupportedMediaType { .. }),
            "{rejection:?}"
        );
    }

    #[tokio::test]
    async fn a_contradicted_parameter_is_another_media_type() {
        let rejection = super::extract::<JsonAs<Order, u32>>(request(
            "application/vnd.acme.order+json; version=1",
            b"7",
        ))
        .await
        .expect_err("version 1 is not the declared version 2");
        assert!(
            matches!(rejection, BodyRejection::UnsupportedMediaType { .. }),
            "{rejection:?}"
        );
    }

    #[tokio::test]
    async fn malformed_json_is_a_syntax_rejection() {
        let rejection =
            super::extract::<JsonAs<Order, u32>>(request("application/vnd.acme.order+json", b"7,"))
                .await
                .expect_err("not JSON");
        assert!(
            matches!(rejection, BodyRejection::Syntax { .. }),
            "{rejection:?}"
        );
    }

    #[tokio::test]
    async fn json_of_the_wrong_shape_is_a_schema_rejection_at_the_root() {
        let rejection = super::extract::<JsonAs<Order, u32>>(request(
            "application/vnd.acme.order+json",
            br#""seven""#,
        ))
        .await
        .expect_err("a string is not a `u32`");
        let BodyRejection::Schema { failures } = rejection else {
            panic!("{rejection:?}");
        };
        assert_eq!(failures.keys().collect::<Vec<_>>(), [""]);
    }

    #[tokio::test]
    async fn a_broken_bound_is_a_schema_rejection_at_its_pointer() {
        let rejection = super::extract::<JsonAs<Order, Quantity>>(request(
            "application/vnd.acme.order+json",
            br#"{"count":0}"#,
        ))
        .await
        .expect_err("a count of zero breaks the bound");
        let BodyRejection::Schema { failures } = rejection else {
            panic!("{rejection:?}");
        };
        assert_eq!(failures.keys().collect::<Vec<_>>(), ["/count"]);
    }

    /// `serde` reads a struct from an array of its members, which `JsonAs`
    /// refuses as `Json` does; a sequence type still reads one.
    #[tokio::test]
    async fn an_array_for_a_struct_is_a_schema_rejection_at_the_root() {
        let rejection = super::extract::<JsonAs<Order, Quantity>>(request(
            "application/vnd.acme.order+json",
            b"[5]",
        ))
        .await
        .expect_err("an array is not an object");
        let BodyRejection::Schema { failures } = rejection else {
            panic!("{rejection:?}");
        };
        assert_eq!(failures.keys().collect::<Vec<_>>(), [""]);
        assert!(
            failures[""].contains("expected a JSON object"),
            "{failures:?}"
        );

        let counts = super::extract::<JsonAs<Order, Vec<u32>>>(request(
            "application/vnd.acme.order+json",
            b"[5]",
        ))
        .await
        .expect("an array is a `Vec`");
        assert_eq!(counts.into_inner(), [5]);
    }

    #[test]
    fn describes_its_own_media_type_over_the_schema_of_its_document() {
        assert_eq!(JsonAs::<Order, u32>::media_types(), [Order::MEDIA_TYPE]);

        let mut registry = Registry::default();
        let mut operation = OperationCx::new(&mut registry);
        JsonAs::<Order, u32>::describe(&mut operation);
        let Some(RefOr::Item(body)) = operation.finish().request_body else {
            panic!("describing the body declares no request body");
        };

        assert_eq!(
            body.content.keys().map(String::as_str).collect::<Vec<_>>(),
            [Order::MEDIA_TYPE]
        );
        assert_eq!(
            body.content[Order::MEDIA_TYPE]
                .as_item()
                .expect("inline")
                .schema,
            Some(Registry::default().resolve::<u32>())
        );
        assert_eq!(body.required, Some(true));
    }
}

/// A struct is read only from an object wherever the document holds one, not
/// only at its top level: `serde` would read one from an array of its members.
#[cfg(feature = "json")]
mod a_nested_json_struct_is_read_only_from_an_object {
    use std::collections::BTreeMap;

    use crate::{
        error::rejection::BodyRejection,
        extract::{body::json, strict_json},
    };

    /// Every member defaults, so `serde` reads `[]` as `{}`.
    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Patch {
        #[serde(default)]
        note: Option<String>,
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Outer {
        inner: Patch,
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Deep {
        outer: Outer,
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Maybe {
        inner: Option<Patch>,
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Wrapped(Patch);

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct HoldsWrapped {
        inner: Wrapped,
    }

    /// A tuple struct is an array, so it is still read from one.
    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Pair(u32, u32);

    #[derive(Debug, PartialEq, serde::Deserialize)]
    enum Change {
        Replace(Patch),
        Edit {
            #[serde(default)]
            note: Option<String>,
        },
        Move(Patch, u32),
    }

    /// The 422 `Json` answers `body` with, which must be a refused array,
    /// keyed at the array's own pointer.
    fn refused<T: serde::de::DeserializeOwned + std::fmt::Debug>(body: &str, at: &str) {
        let failure = strict_json::from_slice::<T>(body.as_bytes())
            .expect_err(&format!("{body}: an array is not an object"));
        let BodyRejection::Schema { failures } = json::rejection(failure) else {
            panic!("{body}: expected a 422");
        };
        assert_eq!(failures.keys().collect::<Vec<_>>(), [at], "{body}");
        let detail = &failures[at];
        assert!(
            detail.contains("expected a JSON object"),
            "{body}: {detail}"
        );
    }

    #[test]
    fn an_array_for_a_nested_struct_is_refused_at_its_pointer() {
        refused::<Outer>(r#"{"inner": []}"#, "/inner");
        refused::<Deep>(r#"{"outer": {"inner": [null]}}"#, "/outer/inner");
        refused::<Maybe>(r#"{"inner": []}"#, "/inner");
        refused::<HoldsWrapped>(r#"{"inner": []}"#, "/inner");
        refused::<Vec<Patch>>("[{}, []]", "/1");
        refused::<BTreeMap<String, Patch>>(r#"{"a": []}"#, "/a");
        refused::<(Patch,)>("[[]]", "/0");
        refused::<Change>(r#"{"Replace": []}"#, "/Replace");
        refused::<Change>(r#"{"Edit": []}"#, "/Edit");
        refused::<Change>(r#"{"Move": [[], 1]}"#, "/Move/0");
    }

    /// The refusal is only for well-formed JSON, as at the top level.
    #[test]
    fn a_malformed_document_holding_an_array_is_still_malformed() {
        for body in [r#"{"inner": [}"#, r#"{"inner": [], "#, r#"{"inner": []} x"#] {
            let failure = strict_json::from_slice::<Outer>(body.as_bytes()).expect_err(body);
            assert!(
                matches!(json::rejection(failure), BodyRejection::Syntax { .. }),
                "{body}: expected a 400"
            );
        }
    }

    #[test]
    fn an_object_and_a_sequence_are_still_read() {
        let patch = || Patch { note: None };
        assert_eq!(
            strict_json::from_slice::<Deep>(br#"{"outer": {"inner": {}}}"#).expect("objects"),
            Deep {
                outer: Outer { inner: patch() }
            }
        );
        assert_eq!(
            strict_json::from_slice::<Maybe>(br#"{"inner": null}"#).expect("null"),
            Maybe { inner: None }
        );
        assert_eq!(
            strict_json::from_slice::<Vec<Vec<u32>>>(b"[[1], []]").expect("arrays"),
            [vec![1], vec![]]
        );
        assert_eq!(
            strict_json::from_slice::<Vec<Pair>>(b"[[1, 2]]").expect("a tuple struct"),
            [Pair(1, 2)]
        );
        assert_eq!(
            strict_json::from_slice::<Change>(br#"{"Move": [{}, 1]}"#).expect("a tuple variant"),
            Change::Move(patch(), 1)
        );
        assert_eq!(
            strict_json::from_slice::<Change>(br#"{"Edit": {"note": "x"}}"#)
                .expect("a struct variant"),
            Change::Edit {
                note: Some("x".to_owned())
            }
        );
        assert_eq!(
            strict_json::from_slice::<HoldsWrapped>(br#"{"inner": {}}"#).expect("a newtype"),
            HoldsWrapped {
                inner: Wrapped(patch())
            }
        );
    }
}

/// A value that does not deserialize is keyed at the JSON Pointer of the
/// position that failed, as a broken bound is, rather than at the root.
#[cfg(feature = "json")]
mod a_json_failure_is_keyed_where_it_happened {
    use std::collections::BTreeMap;

    use crate::{
        error::rejection::BodyRejection,
        extract::{body::json, strict_json},
    };

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct Counted {
        count: u32,
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct Outer {
        inner: Counted,
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Closed {
        count: u32,
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct HoldsClosed {
        inner: Closed,
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct Pair {
        a: Counted,
        b: Counted,
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    enum Change {
        Count(u32),
        Edit { note: String },
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum Either {
        Number(u32),
        Text(String),
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct HoldsEither {
        inner: Either,
    }

    /// A map key read as bytes, as `serde_json` hands them over.
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct ByteKey(Vec<u8>);

    impl<'de> serde::Deserialize<'de> for ByteKey {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;

            impl serde::de::Visitor<'_> for Visitor {
                type Value = ByteKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("bytes")
                }

                fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<ByteKey, E> {
                    Ok(ByteKey(value.to_vec()))
                }
            }

            deserializer.deserialize_bytes(Visitor)
        }
    }

    /// Reads a sequence, swallowing its first element's failure.
    #[derive(Debug)]
    struct Lenient;

    impl<'de> serde::Deserialize<'de> for Lenient {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;

            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Lenient;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("a sequence")
                }

                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut elements: A,
                ) -> Result<Lenient, A::Error> {
                    assert!(elements.next_element::<u32>().is_err(), "a failing element");
                    while elements.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                    Ok(Lenient)
                }
            }

            deserializer.deserialize_seq(Visitor)
        }
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct HoldsLenient {
        a: Lenient,
        b: u32,
    }

    /// Reads a sequence, answering its first element's failure with an error
    /// of its own.
    #[derive(Debug)]
    struct Replacing;

    impl<'de> serde::Deserialize<'de> for Replacing {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;

            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Replacing;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("a sequence")
                }

                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut elements: A,
                ) -> Result<Replacing, A::Error> {
                    elements.next_element::<u32>().map_err(|_| {
                        <A::Error as serde::de::Error>::custom("not a list of counts")
                    })?;
                    Ok(Replacing)
                }
            }

            deserializer.deserialize_seq(Visitor)
        }
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct HoldsReplacing {
        a: Replacing,
    }

    /// Reads a sequence, swallowing its first element's failure and passing
    /// its second's on.
    #[derive(Debug)]
    struct Retrying;

    impl<'de> serde::Deserialize<'de> for Retrying {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;

            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Retrying;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("a sequence")
                }

                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut elements: A,
                ) -> Result<Retrying, A::Error> {
                    assert!(elements.next_element::<u32>().is_err(), "a failing element");
                    elements.next_element::<Counted>()?;
                    Ok(Retrying)
                }
            }

            deserializer.deserialize_seq(Visitor)
        }
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct HoldsRetrying {
        a: Retrying,
    }

    /// The one pointer the 422 `Json` answers `body` with is keyed at.
    fn keyed<T: serde::de::DeserializeOwned + std::fmt::Debug>(body: impl AsRef<[u8]>) -> String {
        let body = body.as_ref();
        let failure = strict_json::from_slice::<T>(body)
            .expect_err(&format!("{}: does not fit", String::from_utf8_lossy(body)));
        let body = String::from_utf8_lossy(body);
        let BodyRejection::Schema { failures } = json::rejection(failure) else {
            panic!("{body}: expected a 422");
        };
        let mut pointers = failures.into_keys();
        let pointer = pointers.next().expect("one failure");
        assert_eq!(pointers.next(), None, "{body}: one failure");
        pointer
    }

    #[test]
    fn a_member_or_an_element_that_does_not_fit_is_keyed_at_its_own_pointer() {
        assert_eq!(
            keyed::<Outer>(r#"{"inner": {"count": "x"}}"#),
            "/inner/count"
        );
        assert_eq!(keyed::<Vec<u32>>(r#"[1, "x"]"#), "/1");
        assert_eq!(
            keyed::<Vec<Counted>>(r#"[{"count": 1}, {"count": -1}]"#),
            "/1/count"
        );
        assert_eq!(keyed::<u32>(r#""x""#), "");
    }

    /// A missing member is the object's failure; an unknown one is its own.
    #[test]
    fn a_missing_member_is_keyed_at_its_object_and_an_unknown_one_at_itself() {
        assert_eq!(keyed::<Outer>(r#"{"inner": {}}"#), "/inner");
        assert_eq!(
            keyed::<HoldsClosed>(r#"{"inner": {"count": 1, "extra": 2}}"#),
            "/inner/extra"
        );
    }

    /// An escaped name is read as unescaped, and written as RFC 6901 escapes
    /// it; a key read as a number or a boolean is named as its text.
    #[test]
    fn a_member_is_named_as_its_key_reads() {
        assert_eq!(
            keyed::<Outer>(r#"{"in\u006eer": {"count": "x"}}"#),
            "/inner/count"
        );
        assert_eq!(
            keyed::<Pair>(r#"{"\u0061": {"count": 1}, "\u0062": {"\u0063ount": "x"}}"#),
            "/b/count"
        );
        assert_eq!(
            keyed::<BTreeMap<String, u32>>(r#"{"a/b~c": "x"}"#),
            "/a~1b~0c"
        );
        assert_eq!(keyed::<BTreeMap<u32, u32>>(r#"{"7": "x"}"#), "/7");
        assert_eq!(keyed::<BTreeMap<bool, u32>>(r#"{"true": "x"}"#), "/true");
    }

    /// A key read as a character or as bytes is named as its text, and bytes
    /// that are not UTF-8 lossily.
    #[test]
    fn a_character_or_byte_key_is_named_as_its_text() {
        assert_eq!(keyed::<BTreeMap<char, u32>>(r#"{"a": "x"}"#), "/a");
        assert_eq!(keyed::<BTreeMap<char, u32>>(r#"{"\u0061": "x"}"#), "/a");
        assert_eq!(keyed::<BTreeMap<ByteKey, u32>>(r#"{"ab": "x"}"#), "/ab");
        assert_eq!(
            keyed::<BTreeMap<ByteKey, u32>>(r#"{"\u0061b": "x"}"#),
            "/ab"
        );
        assert_eq!(
            keyed::<BTreeMap<ByteKey, u32>>(b"{\"a\xffb\": \"x\"}"),
            "/a\u{fffd}b"
        );
        assert_eq!(
            keyed::<BTreeMap<ByteKey, u32>>(r#"{"a\ud800b": "x"}"#),
            "/a\u{fffd}\u{fffd}\u{fffd}b"
        );
    }

    /// A variant's content is keyed under its name once it is read; a variant
    /// the enum does not have, or content of the wrong kind, is the enum's.
    #[test]
    fn a_variant_is_keyed_under_its_name_once_its_content_is_read() {
        assert_eq!(keyed::<Change>(r#"{"Count": "x"}"#), "/Count");
        assert_eq!(keyed::<Change>(r#"{"Edit": {"note": 5}}"#), "/Edit/note");
        assert_eq!(keyed::<Change>(r#"{"Edit": {}}"#), "/Edit");
        assert_eq!(
            keyed::<Change>(r#"{"\u0045dit": {"note": 5}}"#),
            "/Edit/note"
        );
        assert_eq!(keyed::<Change>(r#"{"Edit": 5}"#), "");
        assert_eq!(keyed::<Change>(r#""Edit""#), "");
        assert_eq!(keyed::<Change>(r#"{"Nope": 1}"#), "");
    }

    /// What `serde` buffers is read after the wrappers hand it over, so its
    /// failure is keyed at the buffered value.
    #[test]
    fn a_buffered_value_is_keyed_at_itself() {
        assert_eq!(keyed::<HoldsEither>(r#"{"inner": []}"#), "/inner");
    }

    /// A failure a value swallowed leaves nothing in the next one's pointer.
    #[test]
    fn a_swallowed_failure_is_not_part_of_the_next_pointer() {
        assert_eq!(keyed::<HoldsLenient>(r#"{"a": ["x", 1], "b": "y"}"#), "/b");
        assert_eq!(
            keyed::<HoldsRetrying>(r#"{"a": ["x", {"count": "y"}]}"#),
            "/a/1/count"
        );
        assert_eq!(keyed::<HoldsRetrying>(r#"{"a": ["x", 5]}"#), "/a/1");
    }

    /// An error a visitor answers a failed read with is keyed at that read:
    /// it cannot be told from the read's own error passed on.
    #[test]
    fn an_error_answering_a_failed_read_is_keyed_at_that_read() {
        assert_eq!(keyed::<HoldsReplacing>(r#"{"a": ["x"]}"#), "/a/0");
    }
}

/// The constant a `OneOf` holding a `TextAs` evaluates refuses a marker sharing
/// requests with the body beside it, whatever parameter tells them apart.
#[test]
#[should_panic(expected = "a `TextAs` marker is offered requests the body beside it")]
fn a_shared_text_marker_is_refused_by_the_constant_one_of_evaluates() {
    crate::extract::body::alternative::distinct_text("text/csv; header=present", "TEXT/CSV");
}

/// The control: another subtype is another media type.
#[test]
fn a_text_marker_of_another_subtype_is_admitted() {
    crate::extract::body::alternative::distinct_text("text/csv", "text/plain");
}

/// UTF-8 text under a declared media type reads and refuses as `Text` does,
/// keyed by its own media type rather than `text/plain`.
mod a_text_body_under_a_declared_media_type {
    use kynos_openapi::RefOr;

    use crate::{
        error::rejection::BodyRejection,
        extract::{
            body::{text::Text, text_as::TextAs},
            describe::{Describe, RequestContent},
        },
        http::{HeaderValue, Request, body::Body, header, media::MediaType},
        router::operation::OperationCx,
        schema::registry::Registry,
    };

    /// A text type carrying a parameter, which a request need not repeat.
    #[derive(Debug)]
    struct Csv;

    impl MediaType for Csv {
        const MEDIA_TYPE: &'static str = "text/csv; header=present";
    }

    fn request(content_type: &'static str, body: &'static [u8]) -> Request {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(body)));
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        request
    }

    #[tokio::test]
    async fn reads_its_own_media_type_with_or_without_utf_8_stated() {
        for content_type in [
            "text/csv",
            "TEXT/Csv; header=present",
            "text/csv; charset=utf-8",
            "text/csv; charset=UTF-8; header=present",
        ] {
            let text = super::extract::<TextAs<Csv>>(request(content_type, "a,é\n".as_bytes()))
                .await
                .unwrap_or_else(|rejection| panic!("{content_type}: {rejection:?}"));
            assert_eq!(text.into_inner(), "a,é\n");
        }
    }

    #[tokio::test]
    async fn another_media_type_is_unsupported() {
        for content_type in ["text/plain", "application/json", "text/csv+json"] {
            let rejection = super::extract::<TextAs<Csv>>(request(content_type, b"a,b"))
                .await
                .expect_err(content_type);
            assert!(
                matches!(rejection, BodyRejection::UnsupportedMediaType { .. }),
                "{content_type}: {rejection:?}"
            );
        }

        // And the reverse: `Text` reads no other `text/*` type.
        let rejection = super::extract::<Text>(request("text/csv", b"a,b"))
            .await
            .expect_err("`text/csv` is not `text/plain`");
        assert!(
            matches!(rejection, BodyRejection::UnsupportedMediaType { .. }),
            "{rejection:?}"
        );
    }

    #[tokio::test]
    async fn a_contradicted_parameter_is_unsupported() {
        for content_type in ["text/csv; charset=iso-8859-1", "text/csv; header=absent"] {
            let rejection = super::extract::<TextAs<Csv>>(request(content_type, b"a,b"))
                .await
                .expect_err(content_type);
            assert!(
                matches!(rejection, BodyRejection::UnsupportedMediaType { .. }),
                "{content_type}: {rejection:?}"
            );
        }
    }

    #[tokio::test]
    async fn bytes_that_are_not_utf_8_are_a_syntax_rejection() {
        let rejection = super::extract::<TextAs<Csv>>(request("text/csv", b"caf\xe9"))
            .await
            .expect_err("Latin-1 is not UTF-8");
        assert!(
            matches!(rejection, BodyRejection::Syntax { .. }),
            "{rejection:?}"
        );
    }

    #[test]
    fn describes_its_own_media_type_over_a_string() {
        assert_eq!(TextAs::<Csv>::media_types(), [Csv::MEDIA_TYPE]);

        let mut registry = Registry::default();
        let mut operation = OperationCx::new(&mut registry);
        TextAs::<Csv>::describe(&mut operation);
        let Some(RefOr::Item(body)) = operation.finish().request_body else {
            panic!("describing the body declares no request body");
        };

        assert_eq!(
            body.content.keys().map(String::as_str).collect::<Vec<_>>(),
            [Csv::MEDIA_TYPE]
        );
        assert_eq!(
            body.content[Csv::MEDIA_TYPE]
                .as_item()
                .expect("inline")
                .schema,
            Some(Registry::default().resolve::<String>())
        );
        assert_eq!(body.required, Some(true));
    }
}

/// The constant a `OneOf` holding an `Encoded` evaluates refuses a codec's
/// media type sharing requests with the body beside it.
#[test]
#[should_panic(expected = "a `BodyCodec`'s media type is offered requests the body beside it")]
fn a_shared_codec_media_type_is_refused_by_the_constant_one_of_evaluates() {
    crate::extract::body::alternative::distinct_codec(
        "application/vnd.example.tally; signed=true",
        "Application/Vnd.Example.Tally",
    );
}

/// The control: another subtype is another media type.
#[test]
fn a_codec_media_type_of_another_subtype_is_admitted() {
    crate::extract::body::alternative::distinct_codec(
        "application/vnd.example.tally",
        "application/vnd.example.signed-tally",
    );
}

/// A body in an application's own format is read under every check Kynos
/// applies to its own codecs, which the codec cannot reach to skip.
mod a_body_in_an_outside_format {
    use kynos_openapi::RefOr;

    use crate::{
        error::rejection::BodyRejection,
        extract::{
            body::{
                codec::{BodyCodec, DecodeError, Encoded},
                limit::BodyLimit,
            },
            describe::{Describe, RequestContent},
        },
        http::{HeaderValue, Request, StatusCode, body::Body, header},
        router::operation::OperationCx,
        schema::{
            Schema,
            constraints::{Pointer, Violations},
            registry::Registry,
        },
    };

    /// A decimal tally, carrying a parameter a request need not repeat:
    /// anything but ASCII digits is not the format, and digits past `u64::MAX`
    /// are the format holding a value `u64` cannot.
    struct Tally;

    impl BodyCodec<u64> for Tally {
        const MEDIA_TYPE: &'static str = "application/vnd.example.tally; base=10";

        fn decode(bytes: &bytes::Bytes) -> Result<u64, DecodeError> {
            if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
                return Err(DecodeError::Syntax("not a tally".to_owned()));
            }
            std::str::from_utf8(bytes)
                .unwrap_or_default()
                .parse()
                .map_err(|_| DecodeError::Data("past u64".to_owned()))
        }

        fn encode(value: &u64, out: &mut impl std::io::Write) -> std::io::Result<()> {
            write!(out, "{value}")
        }
    }

    /// A count whose schema declares a bound the codec does not enforce.
    #[derive(Debug)]
    struct Quantity {
        count: u64,
    }

    impl Schema for Quantity {
        fn schema(registry: &mut Registry) -> kynos_openapi::Schema {
            registry.resolve::<u64>()
        }

        fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
            if self.count == 0 {
                violations.report(at.member("count"), "must be at least 1");
            }
        }
    }

    impl BodyCodec<Quantity> for Tally {
        const MEDIA_TYPE: &'static str = "application/vnd.example.tally";

        fn decode(bytes: &bytes::Bytes) -> Result<Quantity, DecodeError> {
            <Self as BodyCodec<u64>>::decode(bytes).map(|count| Quantity { count })
        }

        fn encode(value: &Quantity, out: &mut impl std::io::Write) -> std::io::Result<()> {
            <Self as BodyCodec<u64>>::encode(&value.count, out)
        }
    }

    /// A codec decoding to where its bytes sit, so a test can see whether it
    /// was handed the frame the request carried or a copy of it.
    struct Address;

    impl BodyCodec<u64> for Address {
        const MEDIA_TYPE: &'static str = "application/vnd.example.address";

        fn decode(bytes: &bytes::Bytes) -> Result<u64, DecodeError> {
            Ok(bytes.as_ptr() as u64)
        }

        fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn request(content_type: &'static str, body: &'static [u8]) -> Request {
        let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(body)));
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        request
    }

    #[tokio::test]
    async fn reads_its_own_media_type_with_or_without_its_parameter() {
        for content_type in [
            "application/vnd.example.tally",
            "Application/Vnd.Example.Tally; base=10",
            "application/vnd.example.tally; charset=utf-8",
        ] {
            let tally = super::extract::<Encoded<Tally, u64>>(request(content_type, b"42"))
                .await
                .unwrap_or_else(|rejection| panic!("{content_type}: {rejection:?}"));
            assert_eq!(tally.into_inner(), 42);
        }
    }

    #[tokio::test]
    async fn another_media_type_is_refused_before_the_codec_reads() {
        for content_type in [
            "application/json",
            "application/vnd.example.tally+json",
            "application/vnd.example.tally; base=16",
        ] {
            let rejection = super::extract::<Encoded<Tally, u64>>(request(content_type, b"42"))
                .await
                .expect_err(content_type);
            assert!(
                matches!(rejection, BodyRejection::UnsupportedMediaType { .. }),
                "{content_type}: {rejection:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_body_past_the_limit_is_refused_before_the_codec_reads() {
        let mut request = request("application/vnd.example.tally", b"123456");
        request.extensions_mut().insert(BodyLimit(5));

        let rejection = super::extract::<Encoded<Tally, u64>>(request)
            .await
            .expect_err("six octets past a five-octet limit");
        assert!(
            matches!(rejection, BodyRejection::TooLarge { limit: 5 }),
            "{rejection:?}"
        );
    }

    #[tokio::test]
    async fn an_undecoded_coding_is_refused_before_the_codec_reads() {
        let mut request = request("application/vnd.example.tally", b"42");
        request
            .headers_mut()
            .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));

        let rejection = super::extract::<Encoded<Tally, u64>>(request)
            .await
            .expect_err("a body still under gzip");
        assert!(
            matches!(rejection, BodyRejection::UnsupportedCoding { .. }),
            "{rejection:?}"
        );
    }

    #[tokio::test]
    async fn a_syntax_error_is_a_400() {
        let rejection =
            super::extract::<Encoded<Tally, u64>>(request("application/vnd.example.tally", b"4x2"))
                .await
                .expect_err("not ASCII digits");
        assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        assert!(
            matches!(&rejection, BodyRejection::Syntax { detail } if detail == "not a tally"),
            "{rejection:?}"
        );
    }

    #[tokio::test]
    async fn a_data_error_is_a_422_at_the_root_pointer() {
        let rejection = super::extract::<Encoded<Tally, u64>>(request(
            "application/vnd.example.tally",
            b"18446744073709551616",
        ))
        .await
        .expect_err("one past u64::MAX");
        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let BodyRejection::Schema { failures } = rejection else {
            panic!("{rejection:?}");
        };
        assert_eq!(
            failures.into_iter().collect::<Vec<_>>(),
            [(String::new(), "past u64".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_broken_bound_is_a_422_at_its_pointer() {
        let rejection = super::extract::<Encoded<Tally, Quantity>>(request(
            "application/vnd.example.tally",
            b"0",
        ))
        .await
        .expect_err("a count of zero breaks the bound");
        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let BodyRejection::Schema { failures } = rejection else {
            panic!("{rejection:?}");
        };
        assert_eq!(failures.keys().collect::<Vec<_>>(), ["/count"]);
    }

    /// The single-frame handoff holds through the codec: it reads the frame
    /// the request carried, at the same address, rather than a copy.
    #[tokio::test]
    async fn the_codec_is_handed_the_frame_uncopied() {
        static FRAME: &[u8] = b"one frame";

        let address = super::extract::<Encoded<Address, u64>>(request(
            "application/vnd.example.address",
            FRAME,
        ))
        .await
        .expect("any body decodes")
        .into_inner();
        assert_eq!(address, FRAME.as_ptr() as u64);
    }

    #[test]
    fn describes_its_own_media_type_over_the_schema_of_its_value() {
        assert_eq!(
            Encoded::<Tally, u64>::media_types(),
            [<Tally as BodyCodec<u64>>::MEDIA_TYPE]
        );

        let mut registry = Registry::default();
        let mut operation = OperationCx::new(&mut registry);
        Encoded::<Tally, u64>::describe(&mut operation);
        let Some(RefOr::Item(body)) = operation.finish().request_body else {
            panic!("describing the body declares no request body");
        };

        let media_type = <Tally as BodyCodec<u64>>::MEDIA_TYPE;
        assert_eq!(
            body.content.keys().map(String::as_str).collect::<Vec<_>>(),
            [media_type]
        );
        assert_eq!(
            body.content[media_type].as_item().expect("inline").schema,
            Some(Registry::default().resolve::<u64>())
        );
        assert_eq!(body.required, Some(true));
    }
}
