use super::{
    Html, MediaRanges, Png, admits, admitted, charset, is_media_range, is_text, is_utf8_or_unstated,
};

/// A structured syntax suffix is JSON, which is what lets a vendor media
/// type be decoded as the JSON it is.
#[cfg(any(feature = "json", feature = "openapi32"))]
#[test]
fn a_json_suffixed_media_type_is_read_as_json() {
    use super::is_json;

    assert!(is_json("application/json"));
    assert!(is_json("application/json; charset=utf-8"));
    assert!(is_json("APPLICATION/JSON"));
    assert!(is_json("application/vnd.acme.filter+json"));
    // The suffix in any case, and the essence trimmed of whitespace.
    assert!(is_json(" application/vnd.acme.filter+JSON ; version=2"));

    assert!(!is_json("application/xml"));
    assert!(!is_json("text/plain"));
    // A suffix is a suffix of the base type, not of the parameters.
    assert!(!is_json("application/xml; note=+json"));
}

/// A text marker names the `text` type and some subtype, in any case.
#[test]
fn a_text_media_type_is_the_text_type_with_a_subtype() {
    assert!(is_text("text/csv"));
    assert!(is_text("TEXT/Markdown; variant=CommonMark"));
    assert!(is_text(" text / vnd.acme.report ; charset=utf-8"));

    assert!(!is_text("text/"));
    assert!(!is_text("text"));
    assert!(!is_text("texts/csv"));
    assert!(!is_text("application/text"));
    // The type is the type, not a parameter.
    assert!(!is_text("application/csv; type=text/csv"));
}

/// The charset a marker declares is read from its parameters alone, quoted or
/// not, and only UTF-8 is a charset Kynos decodes.
#[test]
fn a_declared_charset_is_read_and_held_to_utf_8() {
    assert_eq!(charset("text/csv"), None);
    assert_eq!(charset("text/csv; header=present"), None);
    assert_eq!(charset("text/csv;charset=UTF-8"), Some(&b"UTF-8"[..]));
    assert_eq!(
        charset("text/csv; header=present; Charset = \"utf-8\""),
        Some(&b"utf-8"[..])
    );
    // A parameter named like it is not it.
    assert_eq!(charset("text/csv; charsets=latin1"), None);

    assert!(is_utf8_or_unstated("text/csv"));
    assert!(is_utf8_or_unstated("text/csv; charset=Utf-8"));
    assert!(!is_utf8_or_unstated("text/csv; charset=iso-8859-1"));
    assert!(!is_utf8_or_unstated("text/csv; charset=utf8"));
}

/// A `;` inside a quoted parameter value separates nothing (RFC 9110 sections
/// 5.6.4 and 5.6.6), so a `charset=` there is part of that value, not a
/// parameter: swept over every list of up to three parameters drawn from an
/// alphabet whose charset, if any, each piece records for itself.
#[test]
fn a_charset_is_read_only_from_a_parameter_outside_quotes() {
    // Each piece as written, and the charset it declares.
    let alphabet: [(&str, Option<&[u8]>); 8] = [
        ("profile=\"a;charset=latin1\"", None),
        ("profile=\"a\\\";charset=latin1\"", None),
        ("note=\"=;\"", None),
        ("header=present", None),
        ("charset=utf-8", Some(b"utf-8")),
        ("Charset = \"iso-8859-1\"", Some(b"iso-8859-1")),
        ("charset=\"a;b\"", Some(b"a;b")),
        (" ", None),
    ];

    let mut level = vec![Vec::new()];
    let mut lists = level.clone();
    for _ in 0..3 {
        level = level
            .iter()
            .flat_map(|list| {
                alphabet.iter().map(move |piece| {
                    let mut list = list.clone();
                    list.push(*piece);
                    list
                })
            })
            .collect();
        lists.extend(level.iter().cloned());
    }

    for list in lists {
        let written = list
            .iter()
            .fold(String::from("text/x.acme"), |text, (piece, _)| {
                format!("{text};{piece}")
            });
        let expected = list.iter().find_map(|&(_, charset)| charset);
        assert_eq!(charset(&written), expected, "`{written}`");
        assert_eq!(
            is_utf8_or_unstated(&written),
            expected.is_none_or(|charset| charset == b"utf-8"),
            "`{written}`"
        );
    }
}

/// What a `BodyCodec`'s `MEDIA_TYPE` must be: a `type/subtype` of RFC 9110
/// tokens, with any parameters in octets a field value may hold.
#[test]
fn a_codec_media_type_is_a_type_and_subtype_of_tokens() {
    use super::is_media_type;

    for admitted in [
        "application/vnd.example.tally",
        "Application/CBOR",
        "application/vnd.example+cbor",
        "text/x-ledger; charset=utf-8",
        " application/msgpack ",
        "application/vnd.example; version=\"2\"",
        "application/!#$%&'*+-.^_`|~09AZaz",
    ] {
        assert!(is_media_type(admitted), "`{admitted}` is a media type");
    }
    for refused in [
        "",
        "application",
        "application/",
        "/cbor",
        "application/cbor/v2",
        "application /cbor",
        "application/cb or",
        "application/cbor(1)",
        "application/cbor{1}",
        "application/\"cbor\"",
        "application/cbor\r\nx-injected: 1",
        "application/cbor; note=caf\u{e9}",
        "; charset=utf-8",
    ] {
        assert!(!is_media_type(refused), "`{refused:?}` is not a media type");
    }
}

/// RFC 9110 section 12.5.1's three shapes of `media-range`, and nothing else
/// with a wildcard in it.
#[test]
fn a_media_range_has_a_wildcard_only_where_the_grammar_does() {
    for range in [
        "*/*",
        "video/*",
        "video/mp4",
        "text/html; charset=utf-8",
        "text/*; charset=utf-8",
        "video/*;",
        "video/mp4 ; codecs=avc1",
        "video/mp4;a=1;;b=\"x;y\\\"z\"",
    ] {
        assert!(is_media_range(range), "`{range}` is a media range");
    }
    for refused in [
        "*/mp4",
        "*",
        "video",
        "video/",
        "/mp4",
        "video/mp4\r\nx: 1",
        "",
        " video/*",
        "video/* ",
        "text/html; charset",
        "text/html; =utf-8",
        "text/html; charset=",
        "text/html; charset = utf-8",
        "video/mp4; codecs=a b",
        "video/mp4; a=\"x",
        "video/mp4; a=\"x\\\"",
        "video/mp4; a=\"x\"y\"",
    ] {
        assert!(
            !is_media_range(refused),
            "`{refused:?}` is not a media range"
        );
    }
}

/// What can go out as a `Content-Type`, held to section 8.3.1 and section
/// 5.6.6's parameters, since the value is read at run time and nothing compiled
/// it. Every entry is admitted by `*/*`, so the verdict is the value's own.
#[test]
fn only_a_well_formed_concrete_media_type_can_be_sent() {
    struct Anything;
    impl MediaRanges for Anything {
        const RANGES: &'static [&'static str] = &["*/*"];
    }

    for sendable in [
        "video/mp4",
        "VIDEO/MP4",
        "video/mp4;",
        "video/mp4 ; codecs=avc1",
        "video/mp4;a=1;;b=\"x;y\\\"z\"",
        "video/mp4; a=\"\"",
    ] {
        assert!(admitted::<Anything>(sendable).is_some(), "`{sendable}`");
    }
    for refused in [
        "",
        "video",
        "video/",
        "*/*",
        "video/*",
        "*/mp4",
        " video/mp4",
        "video/mp4 ",
        "video / mp4",
        "video/mp4/x",
        "video/mp4; codecs",
        "video/mp4; =avc1",
        "video/mp4; codecs = avc1",
        "video/mp4; codecs=",
        "video/mp4; codecs=a b",
        "video/mp4; a=\"unterminated",
        "video/mp4; a=\"x\\\"",
        "video/mp4; a=\"x\"y\"",
        "video/mp4\r\nx-injected: 1",
        "video/caf\u{e9}",
    ] {
        assert!(admitted::<Anything>(refused).is_none(), "`{refused:?}`");
    }
}

/// Every range against every value, against a table written out by hand: a
/// value not listed for a range must be refused by it, which a sample of the
/// pairs could not say.
#[test]
fn a_range_admits_exactly_the_types_it_covers() {
    const VALUES: [&str; 8] = [
        "video/mp4",
        "VIDEO/MP4",
        "video/webm",
        "audio/mp4",
        "video/mp4; codecs=avc1",
        "text/html",
        "text/html; charset=UTF-8",
        "text/html; level=1; charset=\"utf-8\"",
    ];
    const TABLE: [(&str, &[usize]); 7] = [
        ("*/*", &[0, 1, 2, 3, 4, 5, 6, 7]),
        ("video/*", &[0, 1, 2, 4]),
        ("VIDEO/*", &[0, 1, 2, 4]),
        ("video/mp4", &[0, 1, 4]),
        ("Video/MP4", &[0, 1, 4]),
        ("text/html; charset=utf-8", &[6, 7]),
        ("text/*; level=1", &[7]),
    ];

    for (range, covered) in TABLE {
        for (index, value) in VALUES.into_iter().enumerate() {
            assert_eq!(
                admits(range, value),
                covered.contains(&index),
                "`{range}` over `{value}`"
            );
        }
    }
}

/// A marker is the set holding only itself, so one carrying a parameter admits
/// a type only with it.
#[test]
fn a_marker_is_the_set_of_itself() {
    assert_eq!(<Png as MediaRanges>::RANGES, ["image/png"]);
    assert!(admitted::<Png>("image/png").is_some());
    assert!(admitted::<Png>("image/webp").is_none());

    assert!(admitted::<Html>("text/html; charset=utf-8").is_some());
    assert!(admitted::<Html>("text/html").is_none());
}

/// The second range of a set is read too.
#[test]
fn a_set_admits_what_any_of_its_ranges_does() {
    struct Video;
    impl MediaRanges for Video {
        const RANGES: &'static [&'static str] = &["video/*", "application/x-mpegurl"];
    }

    let sent = admitted::<Video>("application/x-mpegurl").expect("the second entry");
    assert_eq!(sent, "application/x-mpegurl");
    assert!(admitted::<Video>("video/mp4").is_some());
    assert!(admitted::<Video>("application/json").is_none());
}
