use super::{
    Range, Ranged, Selection,
    headers::{AcceptRanges, ContentRange},
    rangeable::Rangeable,
    served::{Conditions, Served},
    source::InMemory,
    spec::{self, Ignored, MAX_RANGES, Spec},
};
use crate::{
    error::rejection::RangeRejection,
    extract::{
        body::binary::Binary,
        params::header::{EncodeHeaders, HeaderParams},
    },
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode, header,
        media::OctetStream,
    },
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};
use bytes::Bytes;

// --- The oracle ------------------------------------------------------------

/// RFC 9110 section 14.1.2, transcribed as prose rather than as code.
///
/// Independently constructed, which is the whole of `docs/testing.md`'s parser
/// rule: this reaches its answer by the sentences' own branches — *is the
/// last-pos greater than or equal to the current length*, *is the
/// representation shorter than the specified suffix-length* — where
/// [`spec::resolve`] reaches it by `min` and `saturating_sub`. An oracle that
/// shared those would agree with the resolver wherever both were wrong.
///
/// `complete_length` is one or more. The zero-length case is not resolution at
/// all: section 14.2 permits ignoring the field there, and Kynos does.
fn satisfied(spec: Spec, complete_length: u64) -> Option<(u64, u64)> {
    let last_offset = complete_length - 1;

    match spec {
        // "For a GET request, a valid bytes range-spec is satisfiable if it is
        // either an int-range with a first-pos that is less than the current
        // length of the selected representation [...]"
        Spec::Offsets { first, .. } if first >= complete_length => None,

        // "If the last-pos value is absent, or if the value is greater than or
        // equal to the current length of the representation data, the byte
        // range is interpreted as the remainder of the representation (i.e.,
        // the server replaces the value of last-pos with a value that is one
        // less than the current length of the selected representation)."
        Spec::Offsets { first, last: None } => Some((first, last_offset)),
        Spec::Offsets {
            first,
            last: Some(last),
        } if last >= complete_length => Some((first, last_offset)),
        Spec::Offsets {
            first,
            last: Some(last),
        } => Some((first, last)),

        // "[...] or a suffix-range with a non-zero suffix-length."
        Spec::Suffix { length: 0 } => None,

        // "If the selected representation is shorter than the specified
        // suffix-length, the entire representation is used."
        Spec::Suffix { length } if length >= complete_length => Some((0, last_offset)),
        Spec::Suffix { length } => Some((complete_length - length, last_offset)),
    }
}

/// Every `int-range` with a first offset of 0 through 9 and a last offset that
/// is absent or 0 through 9, and every `suffix-range` of 0 through 9.
fn every_spec() -> Vec<Spec> {
    let mut specs = Vec::new();

    for first in 0..=9_u64 {
        specs.push(Spec::Offsets { first, last: None });
        for last in 0..=9_u64 {
            specs.push(Spec::Offsets {
                first,
                last: Some(last),
            });
        }
    }

    for length in 0..=9_u64 {
        specs.push(Spec::Suffix { length });
    }

    specs
}

/// The whole finite space, against the oracle.
///
/// A sweep rather than a `proptest`: `docs/testing.md` reads the parser rule as
/// asking for an independent oracle rather than for a generator, and where the
/// space closes, enumerating it is the stronger statement — a draw from this
/// space would be a sample of it.
#[test]
fn every_range_spec_resolves_the_way_the_specification_says() {
    for complete_length in 0..=8_u64 {
        for spec in every_spec() {
            // Section 14.2: a server "MAY ignore a Range header field when the
            // selected representation has no content", which Kynos does before
            // resolution is reached -- so nothing is satisfiable at zero.
            let expected = if complete_length == 0 {
                None
            } else {
                satisfied(spec, complete_length)
            };

            assert_eq!(
                spec::resolve(&[spec], complete_length).first().copied(),
                expected,
                "{spec:?} against a representation of {complete_length} bytes"
            );
        }
    }
}

// --- The reasons a field is ignored ----------------------------------------

/// A request head, as the extractor reads one: with no validator, which is
/// what a handler-supplied representation has.
fn read(method: &Method, fields: &[(HeaderName, &str)]) -> Range<Binary<OctetStream>> {
    validated(method, fields, None)
}

/// The same, against a representation whose entity tag is `validator`.
fn validated(
    method: &Method,
    fields: &[(HeaderName, &str)],
    validator: Option<&str>,
) -> Range<Binary<OctetStream>> {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(
            name.clone(),
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }

    Range::read(spec::read(method, &headers, validator))
}

/// The reason each fixture is ignored for, named by an exhaustive match.
///
/// The match catches a variant added without a name; the transcribed list in
/// [`every_reason_a_range_is_ignored_has_a_case`] catches one added without a
/// fixture, which a match arm nothing reaches cannot.
fn ignored_cases() -> Vec<(&'static str, Ignored)> {
    let range = header::RANGE;
    let if_range = header::IF_RANGE;

    let too_many = format!(
        "bytes={}",
        (0..=MAX_RANGES)
            .map(|index| format!("{index}-{index}"))
            .collect::<Vec<_>>()
            .join(",")
    );

    let observed = [
        read(&Method::GET, &[]),
        read(
            &Method::GET,
            &[(range.clone(), "bytes=0-1"), (range.clone(), "bytes=2-3")],
        ),
        read(&Method::POST, &[(range.clone(), "bytes=0-1")]),
        read(
            &Method::GET,
            &[(range.clone(), "bytes=0-1"), (if_range, "\"v1\"")],
        ),
        read(&Method::GET, &[(range.clone(), "items=0-1")]),
        read(&Method::GET, &[(range.clone(), "bytes=nonsense")]),
        read(&Method::GET, &[(range.clone(), too_many.as_str())]),
        read(&Method::GET, &[(range, "bytes=0-1")]),
    ];

    // The last fixture is a satisfiable field against a representation with no
    // content, which is the one reason that is a property of the representation
    // rather than of the request.
    let lengths = [10, 10, 10, 10, 10, 10, 10, 0];

    observed
        .iter()
        .zip(lengths)
        .map(|(range, complete_length)| {
            let Ok(Selection::Whole(reason)) = range.select(complete_length) else {
                panic!("a field that cannot be applied is answered with the whole representation");
            };

            let name = match reason {
                Ignored::Absent => "Absent",
                Ignored::Repeated => "Repeated",
                Ignored::MethodUndefined => "MethodUndefined",
                Ignored::Conditional => "Conditional",
                Ignored::UnknownUnit => "UnknownUnit",
                Ignored::Malformed => "Malformed",
                Ignored::TooManyRanges => "TooManyRanges",
                Ignored::EmptyRepresentation => "EmptyRepresentation",
            };

            (name, reason)
        })
        .collect()
}

/// The variant names declared inside `enum` in `source`.
fn variants(source: &str, declaration: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != declaration)
        .skip(1)
        .take_while(|line| *line != "}")
        .filter(|line| line.ends_with(','))
        .filter(|line| !line.starts_with("//"))
        .map(|line| line.trim_end_matches(',').to_owned())
        .collect()
}

/// Every reason RFC 9110 section 14.2 gives for not applying a `Range`, counted
/// against the fixtures that reach it.
///
/// The set is closed and the cases are one apiece, which is the idiom
/// `error/rejection/tests.rs` uses for the rejections: a reviewer cannot see
/// the case that was not written.
#[test]
fn every_reason_a_range_is_ignored_has_a_case() {
    const SOURCE: &str = include_str!("spec.rs");

    /// Every variant of `Ignored`, transcribed in declaration order.
    const WITNESSED: [&str; 8] = [
        "Absent",
        "Repeated",
        "MethodUndefined",
        "Conditional",
        "UnknownUnit",
        "Malformed",
        "TooManyRanges",
        "EmptyRepresentation",
    ];

    let declared = variants(SOURCE, "pub enum Ignored {");
    assert_eq!(
        declared, WITNESSED,
        "a reason was added or renamed without a case here"
    );

    let reached: Vec<&str> = ignored_cases().iter().map(|(name, _)| *name).collect();
    assert_eq!(
        reached, WITNESSED,
        "a fixture reached a reason other than the one it was written for"
    );
}

/// Each of them answers with the whole representation, which is what *ignore
/// it* means on the wire.
#[test]
fn an_ignored_range_sends_the_whole_representation() {
    for (name, reason) in ignored_cases() {
        let selection = Selection::Whole(reason);
        assert_eq!(selection.status(), StatusCode::OK, "{name}");
    }
}

// --- The grammar -----------------------------------------------------------

/// What a field value resolves to against a ten-byte representation.
fn selected(value: &str) -> Result<Selection, RangeRejection> {
    Range::<Binary<OctetStream>>::parse(value).select(10)
}

/// Section 14.1.2's own examples, and the whitespace its own example carries.
#[test]
fn the_specifications_own_examples_are_read_as_written() {
    // "bytes= 0-999, 4500-5499, -1000" -- the strict ABNF for `range-set` has
    // no room for the spaces, and section 5.6.1.2 asks a recipient to accept
    // them anyway.
    let requested = spec::parse("bytes= 0-999, 4500-5499, -1000").expect("a readable field");
    assert_eq!(
        requested,
        [
            Spec::Offsets {
                first: 0,
                last: Some(999)
            },
            Spec::Offsets {
                first: 4500,
                last: Some(5499)
            },
            Spec::Suffix { length: 1000 },
        ]
    );

    // An empty list element is skipped, not refused.
    assert_eq!(
        spec::parse("bytes=0-1,,2-3").expect("a readable field"),
        [
            Spec::Offsets {
                first: 0,
                last: Some(1)
            },
            Spec::Offsets {
                first: 2,
                last: Some(3)
            },
        ]
    );

    // The unit is case-insensitive; section 14.1.
    assert_eq!(
        spec::parse("BYTES=0-1").expect("a readable field"),
        [Spec::Offsets {
            first: 0,
            last: Some(1)
        }]
    );
}

/// An invalid `range-spec` invalidates the whole `ranges-specifier`.
///
/// Section 14.1.1: *a ranges-specifier is invalid if it contains any range-spec
/// that is invalid*, so a field is ignored entirely rather than partly honoured.
#[test]
fn one_invalid_spec_invalidates_the_whole_field() {
    for value in [
        // "An int-range is invalid if the last-pos value is present and less
        // than the first-pos."
        "bytes=9-1",
        "bytes=0-1,9-1",
        // Byte ranges do not use `other-range`, so what it would admit does not.
        "bytes=page=1",
        "bytes=+1-2",
        "bytes=1.5-2",
        "bytes=-",
        "bytes=",
        "bytes=,",
        "bytes",
    ] {
        assert_eq!(
            spec::parse(value),
            Err(Ignored::Malformed),
            "`{value}` is not a valid ranges-specifier"
        );
    }
}

/// A unit other than `bytes` is ignored rather than refused.
#[test]
fn an_unknown_range_unit_is_ignored() {
    assert_eq!(spec::parse("items=0-1"), Err(Ignored::UnknownUnit));
    assert_eq!(spec::parse("pages=1-2"), Err(Ignored::UnknownUnit));
}

/// The cap, and the cap the description states, are one number.
#[test]
fn a_range_set_longer_than_the_cap_is_ignored() {
    let within = (0..MAX_RANGES)
        .map(|index| format!("{index}-{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let beyond = format!("{within},99-99");

    assert_eq!(
        spec::parse(&format!("bytes={within}"))
            .expect("the cap is inclusive")
            .len(),
        MAX_RANGES
    );
    assert_eq!(
        spec::parse(&format!("bytes={beyond}")),
        Err(Ignored::TooManyRanges)
    );

    // Section 14.2 names many small ranges as an attack indicator and permits
    // ignoring the field, so the cap is a stated fact rather than a surprise:
    // the first range and seven more is the eight `MAX_RANGES` admits.
    assert_eq!(MAX_RANGES, 8);
    assert_eq!(
        spec::pattern(),
        r"^[Bb][Yy][Tt][Ee][Ss]=[ \t]*(?:\d+-\d*|-\d+)(?:[ \t]*,[ \t]*(?:\d+-\d*|-\d+)){0,7}$"
    );
}

/// The emitted pattern admits what a sender may write, and nothing a sender
/// may not: the recipient lenience of
/// [`the_specifications_own_examples_are_read_as_written`] stays the parser's.
///
/// OpenAPI tooling reads the pattern as ECMA-262, so it is compiled through
/// the translation a declared `pattern` takes: `\d` there is `[0-9]`, where
/// the `regex` dialect's admits any Unicode digit.
#[cfg(feature = "pattern")]
#[test]
fn the_range_pattern_is_the_senders_grammar() {
    let translated =
        kynos_openapi::pattern::translate(&spec::pattern()).expect("the pattern translates");
    let pattern = regex::Regex::new(&translated).expect("the pattern compiles");
    let eight = (0..MAX_RANGES)
        .map(|index| format!("{index}-{index}"))
        .collect::<Vec<_>>()
        .join(", ");

    for admitted in [
        "bytes=0-499",
        "bytes=0-0,-1",
        "bytes= 0-999, 4500-5499, -1000",
        "BYTES=0-1",
        "Bytes=9500-",
        "bytes=0-1\t,\t2-3",
        &format!("bytes={eight}"),
    ] {
        assert!(pattern.is_match(admitted), "`{admitted}` is refused");
    }

    for refused in [
        "bytes=0-1,,2-3",
        "bytes=,0-1",
        "bytes=0-1,",
        "bytes=",
        "bytes =0-1",
        "items=0-1",
        "bytes=0-1\n,2-3",
        "bytes=١-٢",
        &format!("bytes={eight}, 99-99"),
    ] {
        assert!(!pattern.is_match(refused), "`{refused}` is admitted");
    }
}

/// Section 14.1.2: *recipients MUST anticipate potentially large decimal
/// numerals and prevent parsing errors due to integer conversion overflows.*
///
/// All three saturations are semantically right, which is why saturating is not
/// a shortcut: an enormous `first-pos` is past the end and unsatisfiable, an
/// enormous `last-pos` clamps to the end, and an enormous `suffix-length`
/// exceeds the representation and selects the whole of it.
#[test]
fn an_enormous_decimal_saturates_rather_than_failing_to_parse() {
    let huge = "99999999999999999999999";

    assert_eq!(
        spec::parse(&format!("bytes={huge}-")),
        Ok(vec![Spec::Offsets {
            first: u64::MAX,
            last: None
        }])
    );
    assert_eq!(
        selected(&format!("bytes={huge}-")),
        Err(RangeRejection::NotSatisfiable {
            complete_length: 10
        })
    );

    assert_eq!(
        selected(&format!("bytes=0-{huge}")),
        Ok(Selection::Part {
            first: 0,
            last: 9,
            complete_length: 10
        })
    );

    assert_eq!(
        selected(&format!("bytes=-{huge}")),
        Ok(Selection::Part {
            first: 0,
            last: 9,
            complete_length: 10
        })
    );
}

/// A suffix of zero bytes is unsatisfiable, which is a 416 rather than an empty
/// 206.
#[test]
fn a_zero_length_suffix_is_not_satisfiable() {
    assert_eq!(
        selected("bytes=-0"),
        Err(RangeRejection::NotSatisfiable {
            complete_length: 10
        })
    );
}

/// A representation with no content is answered whole, not with a 416.
#[test]
fn a_zero_length_representation_ignores_the_field() {
    let range = Range::<Binary<OctetStream>>::parse("bytes=0-1");

    assert_eq!(
        range.select(0),
        Ok(Selection::Whole(Ignored::EmptyRepresentation))
    );
}

// --- What reaches the wire -------------------------------------------------

/// One field value off a response.
fn field(response: &Response, name: &HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().expect("a printable field").to_owned())
}

/// A whole representation is a 200 that advertises the unit, and carries no
/// `Content-Range` — section 14.4 gives the field no meaning on a 200.
#[test]
fn a_whole_representation_advertises_the_unit_and_nothing_else() {
    let whole = Binary::<OctetStream>::new(&b"0123456789"[..]);
    let ranged = Range::parse("cheeses=0-1")
        .apply(whole)
        .expect("an ignored field is not a failure");

    assert_eq!(ranged.selection(), Selection::Whole(Ignored::UnknownUnit));
    assert_eq!(ranged.body().octets(), &b"0123456789"[..]);

    let response = ranged.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        field(&response, &header::ACCEPT_RANGES).as_deref(),
        Some("bytes")
    );
    assert_eq!(field(&response, &header::CONTENT_RANGE), None);
}

/// A 206 names the part it actually holds.
///
/// The octets and the field are asserted together, because the failure worth
/// catching is the two disagreeing: section 14.4 says a recipient of an invalid
/// `Content-Range` MUST NOT recombine, and a client that recombines a correct
/// field naming the wrong octets corrupts the representation silently.
#[test]
fn a_partial_representation_names_the_bytes_it_actually_carries() {
    let whole = Binary::<OctetStream>::new(&b"0123456789"[..]);
    let ranged = Range::parse("bytes=2-4").apply(whole).expect("2-4 of 10");

    let Selection::Part {
        first,
        last,
        complete_length,
    } = ranged.selection()
    else {
        panic!("a satisfiable field selects a part");
    };

    let octets = ranged.body().octets().clone();
    assert_eq!(octets, &b"234"[..]);
    assert_eq!(u64::try_from(octets.len()), Ok(last - first + 1));
    assert_eq!(complete_length, 10);

    let response = ranged.into_response();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        field(&response, &header::CONTENT_RANGE).as_deref(),
        Some("bytes 2-4/10")
    );
    assert_eq!(
        field(&response, &header::ACCEPT_RANGES).as_deref(),
        Some("bytes")
    );
    assert_eq!(
        field(&response, &header::CONTENT_TYPE).as_deref(),
        Some(<Binary<OctetStream> as Rangeable>::media_type())
    );
}

/// A suffix reaches the last bytes, and a first-only range the remainder.
#[test]
fn a_suffix_and_an_open_ended_range_reach_the_end() {
    let whole = Binary::<OctetStream>::new(&b"0123456789"[..]);

    for (value, expected, enclosed) in [
        ("bytes=-3", "789", "bytes 7-9/10"),
        ("bytes=7-", "789", "bytes 7-9/10"),
        ("bytes=0-0", "0", "bytes 0-0/10"),
        ("bytes=0-99", "0123456789", "bytes 0-9/10"),
        ("bytes=-99", "0123456789", "bytes 0-9/10"),
    ] {
        let ranged = Range::parse(value)
            .apply(whole.clone())
            .expect("a satisfiable field");

        assert_eq!(ranged.body().octets(), expected.as_bytes(), "{value}");
        assert_eq!(
            field(&ranged.into_response(), &header::CONTENT_RANGE).as_deref(),
            Some(enclosed),
            "{value}"
        );
    }
}

/// The first satisfiable spec is the one served, and the rest are left for the
/// client to ask for again.
///
/// Section 14.2: *the above does not imply that a server will send all
/// requested ranges.* What is missing is `multipart/byteranges`, and a 206 is
/// self-descriptive in the meantime.
#[test]
fn a_multi_range_field_is_answered_with_its_first_satisfiable_part() {
    assert_eq!(
        selected("bytes=2-3,6-7"),
        Ok(Selection::Part {
            first: 2,
            last: 3,
            complete_length: 10
        })
    );

    // An unsatisfiable spec is dropped rather than invalidating the field: it
    // is *valid*, it just selects nothing.
    assert_eq!(
        selected("bytes=99-,6-7"),
        Ok(Selection::Part {
            first: 6,
            last: 7,
            complete_length: 10
        })
    );

    // And a field with nothing satisfiable in it is the 416.
    assert_eq!(
        selected("bytes=99-,50-60"),
        Err(RangeRejection::NotSatisfiable {
            complete_length: 10
        })
    );
}

// --- What the description says ---------------------------------------------

/// `Ranged<T>` declares two statuses and no more.
#[test]
fn a_ranged_response_declares_exactly_the_whole_and_the_part() {
    let described = <Ranged<Binary<OctetStream>> as Responses>::responses(&mut Registry::default());

    let statuses: Vec<&str> = described.responses.keys().map(String::as_str).collect();
    assert_eq!(statuses, ["200", "206"]);
    assert!(described.default_response.is_none());
}

/// The 200 advertises the unit; the 206 advertises it and names the part.
#[test]
fn each_declared_status_carries_the_fields_it_actually_sends() {
    let described = <Ranged<Binary<OctetStream>> as Responses>::responses(&mut Registry::default());

    let headers = |status: &str| {
        let kynos_openapi::RefOr::Item(response) = described
            .responses
            .get(status)
            .unwrap_or_else(|| panic!("{status} is declared"))
        else {
            panic!("{status} is described as a `$ref`");
        };
        response.headers.keys().cloned().collect::<Vec<_>>()
    };

    assert_eq!(headers("200"), ["Accept-Ranges"]);
    assert_eq!(headers("206"), ["Accept-Ranges", "Content-Range"]);
}

/// The pattern a range header is described by, with the shape every one of
/// them shares asserted on the way: required, and `text/plain` content.
fn pattern(header: &kynos_openapi::Header) -> String {
    let (media_type, content) = header.content().expect("described as `text/plain` content");
    assert_eq!(media_type, "text/plain");
    assert_eq!(header.required, Some(true));

    let content = content.as_item().expect("an inline media type");
    let kynos_openapi::Schema::Object(schema) = content.schema.clone().expect("a schema") else {
        panic!("described by a boolean schema");
    };
    schema.pattern.clone().expect("a pattern")
}

/// `Accept-Ranges`'s description, as the group declares it.
fn accept_ranges_header() -> kynos_openapi::Header {
    let advertised = AcceptRanges::response_headers(&mut Registry::default());
    let kynos_openapi::RefOr::Item(header) = advertised
        .get("Accept-Ranges")
        .cloned()
        .expect("the canonical spelling")
    else {
        panic!("described as a `$ref`");
    };
    header
}

/// Both groups are described, and each states the grammar it writes.
#[test]
fn both_groups_describe_the_fields_they_send() {
    assert!(std::hint::black_box(
        <AcceptRanges as HeaderParams>::DESCRIBED
    ));
    assert!(std::hint::black_box(
        <ContentRange as HeaderParams>::DESCRIBED
    ));

    assert_eq!(
        pattern(&ContentRange::satisfied_header()),
        r"^bytes \d+-\d+/\d+$"
    );
    assert_eq!(
        pattern(&ContentRange::unsatisfied_header()),
        r"^bytes \*/\d+$"
    );

    assert_eq!(pattern(&accept_ranges_header()), "^bytes$");
}

/// Each range field's Header Object, whole: the three share one shape and
/// differ in pattern and description, so a description moved onto the wrong
/// field reads as well-formed to every case above.
#[test]
fn each_range_field_is_described_exactly() {
    let described = |pattern: &str, description: &str| {
        serde_json::json!({
            "description": description,
            "required": true,
            "content": { "text/plain": { "schema": { "type": "string", "pattern": pattern } } },
        })
    };
    let json = |header: kynos_openapi::Header| serde_json::to_value(header).expect("JSON");

    assert_eq!(
        json(accept_ranges_header()),
        described(
            "^bytes$",
            "The range units this operation serves, per RFC 9110 section 14.3."
        )
    );
    assert_eq!(
        json(ContentRange::satisfied_header()),
        described(
            r"^bytes \d+-\d+/\d+$",
            "The part of the representation enclosed, and its complete length, per RFC 9110 \
             section 14.4."
        )
    );
    assert_eq!(
        json(ContentRange::unsatisfied_header()),
        described(
            r"^bytes \*/\d+$",
            "The complete length of the selected representation, per RFC 9110 section 15.5.17."
        )
    );
}

/// The field each group writes matches the pattern it declares.
#[test]
fn each_group_writes_a_value_its_own_grammar_admits() {
    let sent = |group: &dyn Fn() -> Result<Vec<(HeaderName, HeaderValue)>, _>| {
        let fields = group().expect("a range group always encodes");
        assert_eq!(fields.len(), 1);
        fields[0].1.to_str().expect("a printable field").to_owned()
    };

    let accept_ranges = sent(&|| AcceptRanges.encode());
    let satisfied = sent(&|| {
        ContentRange::Satisfied {
            first: 42,
            last: 1233,
            complete_length: 1234,
        }
        .encode()
    });
    let unsatisfied = sent(&|| {
        ContentRange::Unsatisfied {
            complete_length: 47022,
        }
        .encode()
    });

    assert_eq!(accept_ranges, "bytes");
    // Section 14.4's own worked example.
    assert_eq!(satisfied, "bytes 42-1233/1234");
    // Section 15.5.17's own worked example.
    assert_eq!(unsatisfied, "bytes */47022");

    // And each is admitted by the pattern its group declares, which is the
    // half of the name the exact values above cannot say. Behind `pattern`,
    // the feature that carries a regular expression engine.
    #[cfg(feature = "pattern")]
    for (written, header) in [
        (accept_ranges, accept_ranges_header()),
        (satisfied, ContentRange::satisfied_header()),
        (unsatisfied, ContentRange::unsatisfied_header()),
    ] {
        let declared = pattern(&header);
        assert!(
            regex::Regex::new(&declared)
                .expect("a declared pattern compiles")
                .is_match(&written),
            "`{written}` is not admitted by `{declared}`"
        );
    }
}

// --- Delivery preconditions ------------------------------------------------

/// The status an untagged ten-octet [`Served`] answers a `GET` carrying
/// `fields` with.
async fn untagged_status(fields: &[(HeaderName, &str)]) -> StatusCode {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(name, HeaderValue::from_str(value).expect("a valid field"));
    }
    let conditions = Conditions {
        method: Method::GET,
        fields: headers,
    };

    Served::<_, OctetStream>::new(InMemory::new(Bytes::from_static(b"0123456789")))
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
        .status()
}

/// The extractor keeps every line of the six fields it reads, in order, and
/// nothing else of the head.
///
/// Every line, because the readers count them: a second `Range` line is
/// [`Ignored::Repeated`], and a second `If-Modified-Since` voids the date.
#[tokio::test]
async fn conditions_keep_every_line_of_the_fields_they_read_and_nothing_else() {
    use crate::extract::FromRequestParts;

    let read = [
        (header::RANGE, "bytes=0-1"),
        (header::RANGE, "bytes=2-3"),
        (header::IF_RANGE, "\"b\""),
        (header::IF_MATCH, "\"a\""),
        (header::IF_MATCH, "\"b\""),
        (header::IF_UNMODIFIED_SINCE, "Sun, 06 Nov 1994 08:49:37 GMT"),
        (header::IF_NONE_MATCH, "\"c\""),
        (header::IF_MODIFIED_SINCE, "Sun, 06 Nov 1994 08:49:37 GMT"),
    ];
    let mut parts = http::Request::new(()).into_parts().0;
    parts
        .headers
        .append(header::ACCEPT, HeaderValue::from_static("*/*"));
    for (name, value) in &read {
        parts.headers.append(name, HeaderValue::from_static(value));
    }

    let Ok(conditions) = Conditions::from_request_parts(&mut parts, &()).await;

    let kept: Vec<(&HeaderName, &str)> = conditions
        .fields
        .iter()
        .map(|(name, value)| (name, value.to_str().expect("visible ASCII")))
        .collect();
    let expected: Vec<(&HeaderName, &str)> =
        read.iter().map(|(name, value)| (name, *value)).collect();
    assert_eq!(kept, expected);
    assert_eq!(
        parts.headers.len(),
        read.len() + 1,
        "extracting reads the head and leaves it whole"
    );
}

/// No precondition field, no allocation: the map stays the empty one.
#[tokio::test]
async fn conditions_over_a_head_without_the_fields_hold_an_empty_map() {
    use crate::extract::FromRequestParts;

    let mut parts = http::Request::new(()).into_parts().0;
    parts
        .headers
        .append(header::ACCEPT, HeaderValue::from_static("*/*"));

    let Ok(conditions) = Conditions::from_request_parts(&mut parts, &()).await;

    assert!(conditions.fields.is_empty());
    assert_eq!(conditions.fields.capacity(), 0);
}

/// Section 13.1.2: `If-None-Match: *` is false wherever a current
/// representation exists, which a readable length proves with or without an
/// entity tag. A list of tags still needs a tag to name.
#[tokio::test]
async fn an_untagged_representation_answers_any_with_not_modified() {
    assert_eq!(
        untagged_status(&[(header::IF_NONE_MATCH, "*")]).await,
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        untagged_status(&[(header::IF_NONE_MATCH, "\"r3\"")]).await,
        StatusCode::OK
    );
}

/// When the representation [`tagged_status`] serves last changed:
/// `Sun, 06 Nov 1994 08:49:37 GMT`.
const MODIFIED: std::time::Duration = std::time::Duration::from_secs(784_111_777);

/// The status a ten-octet [`Served`] tagged `"b"` and last modified at
/// [`MODIFIED`] answers a `GET` carrying `fields` with, each pair one field
/// line.
async fn tagged_status(fields: &[(HeaderName, &str)]) -> StatusCode {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(name, HeaderValue::from_str(value).expect("a valid field"));
    }
    let conditions = Conditions {
        method: Method::GET,
        fields: headers,
    };

    Served::<_, OctetStream>::new(InMemory::new(Bytes::from_static(b"0123456789")))
        .etag(crate::http::etag::ETag::strong("b"))
        .last_modified(std::time::SystemTime::UNIX_EPOCH + MODIFIED)
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
        .status()
}

/// Section 13.1.2: a false `If-None-Match` answers 304 only to GET and HEAD,
/// and 412 to any other method, since a 304 would tell the client its unsafe
/// request was a cache validation that succeeded.
#[tokio::test]
async fn a_false_if_none_match_on_an_unsafe_method_answers_412() {
    for (method, expected) in [
        (Method::GET, StatusCode::NOT_MODIFIED),
        (Method::HEAD, StatusCode::NOT_MODIFIED),
        (Method::POST, StatusCode::PRECONDITION_FAILED),
        (Method::PUT, StatusCode::PRECONDITION_FAILED),
        (Method::DELETE, StatusCode::PRECONDITION_FAILED),
    ] {
        for named in ["\"b\"", "*"] {
            let mut fields = HeaderMap::new();
            fields.insert(header::IF_NONE_MATCH, HeaderValue::from_static(named));
            let conditions = Conditions {
                method: method.clone(),
                fields,
            };

            let status = Served::<_, OctetStream>::new(InMemory::new(Bytes::from_static(b"0123")))
                .etag(crate::http::etag::ETag::strong("b"))
                .deliver(&conditions)
                .await
                .expect("an in-memory source cannot fail")
                .status();

            assert_eq!(status, expected, "{method} with If-None-Match: {named}");
        }
    }
}

/// What a ten-octet [`Served`] tagged `"b"` and last modified at [`MODIFIED`]
/// answers `method` carrying `fields` with: its status, its field names in
/// order, and whether its body is known to be empty.
async fn delivered_head(
    method: Method,
    fields: &[(HeaderName, &'static str)],
) -> (StatusCode, Vec<String>, bool) {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(name, HeaderValue::from_static(value));
    }
    let conditions = Conditions {
        method,
        fields: headers,
    };

    let response = Served::<_, OctetStream>::new(InMemory::new(Bytes::from_static(b"0123456789")))
        .etag(crate::http::etag::ETag::strong("b"))
        .last_modified(std::time::SystemTime::UNIX_EPOCH + MODIFIED)
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
        .into_response();

    let names = response
        .headers()
        .keys()
        .map(|name| name.as_str().to_owned())
        .collect();
    let empty = http_body::Body::is_end_stream(response.body())
        && http_body::Body::size_hint(response.body()).exact() == Some(0);
    (response.status(), names, empty)
}

/// Both 412s a delivery answers with carry nothing but their status, and its
/// 304 carries its two validators and no content. Each of the three is built
/// at its own site, so each is held to its whole head.
#[tokio::test]
async fn a_refused_delivery_is_bare_and_a_304_carries_its_validators() {
    let bare = (StatusCode::PRECONDITION_FAILED, Vec::<String>::new(), true);

    assert_eq!(
        delivered_head(Method::GET, &[(header::IF_MATCH, "\"a\"")]).await,
        bare,
        "a failed If-Match"
    );
    assert_eq!(
        delivered_head(Method::PUT, &[(header::IF_NONE_MATCH, "\"b\"")]).await,
        bare,
        "a false If-None-Match on an unsafe method"
    );
    assert_eq!(
        delivered_head(Method::GET, &[(header::IF_NONE_MATCH, "\"b\"")]).await,
        (
            StatusCode::NOT_MODIFIED,
            vec!["etag".to_owned(), "last-modified".to_owned()],
            true
        ),
    );
}

/// A `*` line beside a tag line matches by itself, so an untagged
/// representation still answers 304 where the tag line alone would not.
#[tokio::test]
async fn a_star_line_beside_a_tag_line_matches_by_itself() {
    assert_eq!(
        untagged_status(&[
            (header::IF_NONE_MATCH, "\"a\""),
            (header::IF_NONE_MATCH, "*"),
        ])
        .await,
        StatusCode::NOT_MODIFIED
    );
}

/// Section 5.3: two `If-None-Match` lines are one list, so a tag on the second
/// line matches as surely as one on the first.
#[tokio::test]
async fn every_if_none_match_line_is_read() {
    assert_eq!(
        tagged_status(&[
            (header::IF_NONE_MATCH, "\"a\""),
            (header::IF_NONE_MATCH, "\"b\""),
        ])
        .await,
        StatusCode::NOT_MODIFIED
    );

    // The control: neither line names the current tag.
    assert_eq!(
        tagged_status(&[
            (header::IF_NONE_MATCH, "\"a\""),
            (header::IF_NONE_MATCH, "\"c\""),
        ])
        .await,
        StatusCode::OK
    );
}

/// Section 13.1.3: "A recipient MUST ignore the If-Modified-Since header field
/// if ... the field value has more than one member", and two lines are two.
#[tokio::test]
async fn a_repeated_if_modified_since_is_ignored() {
    const SENT: &str = "Sun, 06 Nov 1994 08:49:37 GMT";

    assert_eq!(
        tagged_status(&[
            (header::IF_MODIFIED_SINCE, SENT),
            (header::IF_MODIFIED_SINCE, SENT),
        ])
        .await,
        StatusCode::OK
    );

    // The control: the same date on one line is honoured.
    assert_eq!(
        tagged_status(&[(header::IF_MODIFIED_SINCE, SENT)]).await,
        StatusCode::NOT_MODIFIED
    );
}

/// A tag that is no `entity-tag` answers 500 whatever the request's
/// preconditions, rather than an untagged response or a precondition
/// evaluated as though the representation had no tag.
#[tokio::test]
async fn an_unencodable_tag_answers_500_before_any_precondition() {
    for fields in [
        vec![],
        // Would be a 304 for any representation.
        vec![(header::IF_NONE_MATCH, "*")],
        // Would be a 412 for an untagged one.
        vec![(header::IF_MATCH, "\"a\"")],
        // Would be a 206.
        vec![(header::RANGE, "bytes=0-1")],
    ] {
        let mut headers = HeaderMap::new();
        for (name, value) in &fields {
            headers.append(name, HeaderValue::from_static(value));
        }
        let conditions = Conditions {
            method: Method::GET,
            fields: headers,
        };

        let response =
            Served::<_, OctetStream>::new(InMemory::new(Bytes::from_static(b"0123456789")))
                .etag(crate::http::etag::ETag::strong("a\"b"))
                .deliver(&conditions)
                .await
                .expect("an in-memory source cannot fail")
                .into_response();

        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{fields:?}"
        );
        assert_eq!(response.headers().get(header::ETAG), None, "{fields:?}");
    }
}

/// A `Cache-Control` that is no field value answers 500 in place of every
/// response that would carry it, rather than that response without it; a 412
/// carries none, so it is answered as it would be.
#[tokio::test]
async fn an_unencodable_cache_control_answers_500_where_it_would_be_sent() {
    for (fields, expected) in [
        (vec![], StatusCode::INTERNAL_SERVER_ERROR),
        (
            vec![(header::IF_NONE_MATCH, "\"a\"")],
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            vec![(header::RANGE, "bytes=0-1")],
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            vec![(header::IF_MATCH, "\"b\"")],
            StatusCode::PRECONDITION_FAILED,
        ),
    ] {
        let mut headers = HeaderMap::new();
        for (name, value) in &fields {
            headers.append(name, HeaderValue::from_static(value));
        }
        let conditions = Conditions {
            method: Method::GET,
            fields: headers,
        };

        let response =
            Served::<_, OctetStream>::new(InMemory::new(Bytes::from_static(b"0123456789")))
                .etag(crate::http::etag::ETag::strong("a"))
                .cache_control("max-age=60\n")
                .deliver(&conditions)
                .await
                .expect("an in-memory source cannot fail")
                .into_response();

        assert_eq!(response.status(), expected, "{fields:?}");
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            None,
            "{fields:?}"
        );
        if expected == StatusCode::INTERNAL_SERVER_ERROR {
            // The shared refusal itself, not the file's bytes under its status.
            let refusal = crate::response::headers::refused();
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE),
                refusal.headers().get(header::CONTENT_TYPE),
                "{fields:?}"
            );
            assert_eq!(body(response).await, body(refusal).await, "{fields:?}");
        }
    }
}

/// The body a response carries, read to its end.
async fn body(response: Response) -> Bytes {
    use http_body_util::BodyExt;

    response
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes()
}

/// A media type marker deriving nothing beyond the one trait `Served` requires.
struct Bare;

impl crate::http::media::MediaType for Bare {
    const MEDIA_TYPE: &'static str = "application/octet-stream";
}

/// The marker sits only in a `PhantomData`, so `Debug` on `Served` and
/// `Delivery` is bounded on what they hold and prints that, leaving the marker
/// out.
#[tokio::test]
async fn a_marker_deriving_nothing_leaves_served_and_delivery_debug() {
    let served = Served::<_, Bare>::new(InMemory::new(Bytes::from_static(b"01")));
    assert_eq!(
        format!("{served:?}"),
        r#"Served { source: InMemory(b"01"), content_type: Declared, etag: None, last_modified: None, cache_control: None, disposition: None, .. }"#
    );

    let conditions = Conditions {
        method: Method::GET,
        fields: HeaderMap::new(),
    };
    let delivery = served
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail");
    let printed = format!("{delivery:?}");
    assert!(
        printed.starts_with("Delivery { response: Response { status: 200,"),
        "{printed}"
    );
    assert!(printed.ends_with(", .. }"), "{printed}");
}

/// The representation sits only in a `PhantomData`, so `Range` is bounded on
/// nothing and prints the `range-set` alone.
#[test]
fn a_representation_deriving_nothing_leaves_range_its_traits() {
    fn is_eq<T: Eq>() {}

    let range = Range::<Bare>::parse("bytes=0-1");
    assert_eq!(range.clone(), range);
    assert_ne!(range, Range::<Bare>::parse("bytes=1-2"));
    assert_eq!(
        format!("{range:?}"),
        "Range { requested: Ok([Offsets { first: 0, last: Some(1) }]), .. }"
    );
    is_eq::<Range<Bare>>();
}

// --- A delivery choosing its media type per request ------------------------

/// What an operation serving either a video container or a playlist describes.
struct Video;

impl crate::http::media::MediaRanges for Video {
    const RANGES: &'static [&'static str] = &["video/*", "application/x-mpegurl"];
}

/// What a ten-octet delivery tagged `"b"` and given `content_type` answers
/// `method` carrying `fields` with.
async fn detected(
    method: Method,
    content_type: &str,
    fields: &[(HeaderName, &'static str)],
) -> Response {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(name, HeaderValue::from_static(value));
    }
    let conditions = Conditions {
        method,
        fields: headers,
    };

    Served::<_, Video>::new(InMemory::new(Bytes::from_static(b"0123456789")))
        .etag(crate::http::etag::ETag::strong("b"))
        .content_type(content_type)
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
        .into_response()
}

fn content_type_of(response: &Response) -> Option<&str> {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().expect("visible ASCII"))
}

/// The status and the label of each status a delivery can answer with: the two
/// carrying the representation say the chosen type, and each other keeps what
/// the range engine gives it, whatever was chosen.
#[tokio::test]
async fn a_detected_type_labels_the_200_and_206_and_no_other_status() {
    type Fields<'a> = &'a [(HeaderName, &'static str)];

    let range = [(header::RANGE, "bytes=0-1")];
    let current = [(header::IF_NONE_MATCH, "\"b\"")];
    let stale = [(header::IF_MATCH, "\"a\"")];
    let unsatisfiable = [(header::RANGE, "bytes=99-")];

    let cases: [(Method, Fields<'_>, StatusCode, Option<&str>); 6] = [
        (Method::GET, &[], StatusCode::OK, Some("video/mp4")),
        (Method::HEAD, &[], StatusCode::OK, Some("video/mp4")),
        (
            Method::GET,
            &range,
            StatusCode::PARTIAL_CONTENT,
            Some("video/mp4"),
        ),
        (Method::GET, &current, StatusCode::NOT_MODIFIED, None),
        (Method::GET, &stale, StatusCode::PRECONDITION_FAILED, None),
        (
            Method::GET,
            &unsatisfiable,
            StatusCode::RANGE_NOT_SATISFIABLE,
            Some("application/problem+json"),
        ),
    ];

    for (method, fields, status, label) in cases {
        let response = detected(method.clone(), "video/mp4", fields).await;
        assert_eq!(response.status(), status, "{method} {fields:?}");
        assert_eq!(content_type_of(&response), label, "{method} {fields:?}");
    }
}

/// A type the set does not admit is refused only where it would be sent: the
/// 200 and 206 become the shared 500's problem document, while a 304, a 412 and
/// a 416, which never carry it, are answered as they would be.
#[tokio::test]
async fn an_inadmissible_type_refuses_only_the_responses_carrying_it() {
    let refused = detected(Method::GET, "image/png", &[]).await;
    assert_eq!(refused.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        content_type_of(&refused),
        Some("application/problem+json"),
        "the refused type never reaches the wire"
    );

    let range = [(header::RANGE, "bytes=0-1")];
    assert_eq!(
        detected(Method::GET, "image/png", &range).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
    );

    for (fields, status) in [
        ([(header::IF_NONE_MATCH, "\"b\"")], StatusCode::NOT_MODIFIED),
        (
            [(header::IF_MATCH, "\"a\"")],
            StatusCode::PRECONDITION_FAILED,
        ),
        (
            [(header::RANGE, "bytes=99-")],
            StatusCode::RANGE_NOT_SATISFIABLE,
        ),
    ] {
        assert_eq!(
            detected(Method::GET, "image/png", &fields).await.status(),
            status,
        );
    }
}

/// The set is the `content` of the 200 and the 206 alone, each entry a key with
/// nothing constraining its bytes; the rest describe what they carry today.
#[test]
fn a_set_is_described_as_the_content_keys_of_the_200_and_206() {
    let described = <super::served::Delivery<Video> as Responses>::responses(&mut Registry::new());
    let document = serde_json::to_value(&described).expect("responses serialize");

    for status in ["200", "206"] {
        assert_eq!(
            document[status]["content"],
            serde_json::json!({
                "application/x-mpegurl": {"schema": {}},
                "video/*": {"schema": {}},
            }),
            "{status}"
        );
    }
    for status in ["304", "412"] {
        assert!(document[status].get("content").is_none(), "{status}");
    }
    let problem: Vec<&String> = document["416"]["content"]
        .as_object()
        .expect("a 416 carries a problem document")
        .keys()
        .collect();
    assert_eq!(problem, ["application/problem+json"]);
}
