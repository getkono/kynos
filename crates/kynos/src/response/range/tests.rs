use super::{
    Range, Ranged, Selection,
    headers::{AcceptRanges, ContentRange},
    rangeable::Rangeable,
    spec::{self, Ignored, MAX_RANGES, Spec},
};
use crate::{
    error::rejection::RangeRejection,
    extract::{body::binary::Binary, media::OctetStream, params::header::HeaderParams},
    http::{HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

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

/// A request head, as the extractor reads one.
fn read(method: &Method, fields: &[(HeaderName, &str)]) -> Range<Binary<OctetStream>> {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(
            name.clone(),
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }

    Range::read(spec::read(method, &headers))
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
    // ignoring the field, so the cap is a stated fact rather than a surprise.
    assert!(spec::pattern().contains(&format!("{{0,{}}}", MAX_RANGES - 1)));
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

/// Both groups are described, and each states the grammar it writes.
#[test]
fn both_groups_describe_the_fields_they_send() {
    assert!(std::hint::black_box(
        <AcceptRanges as HeaderParams>::DESCRIBED
    ));
    assert!(std::hint::black_box(
        <ContentRange as HeaderParams>::DESCRIBED
    ));

    let pattern = |header: &kynos_openapi::Header| {
        let (media_type, content) = header.content().expect("described as `text/plain` content");
        assert_eq!(media_type, "text/plain");
        assert_eq!(header.required, Some(true));

        let kynos_openapi::Schema::Object(schema) = content.schema.clone().expect("a schema")
        else {
            panic!("described by a boolean schema");
        };
        schema.pattern.clone().expect("a pattern")
    };

    assert_eq!(
        pattern(&ContentRange::satisfied_header()),
        r"^bytes \d+-\d+/\d+$"
    );
    assert_eq!(
        pattern(&ContentRange::unsatisfied_header()),
        r"^bytes \*/\d+$"
    );

    let advertised = AcceptRanges::response_headers(&mut Registry::default());
    let kynos_openapi::RefOr::Item(header) = advertised
        .get("Accept-Ranges")
        .expect("the canonical spelling")
    else {
        panic!("described as a `$ref`");
    };
    assert_eq!(pattern(header), "^bytes$");
}

/// The field each group writes matches the pattern it declares.
#[test]
fn each_group_writes_a_value_its_own_grammar_admits() {
    let sent = |group: &dyn Fn() -> Vec<(HeaderName, HeaderValue)>| {
        let fields = group();
        assert_eq!(fields.len(), 1);
        fields[0].1.to_str().expect("a printable field").to_owned()
    };

    assert_eq!(sent(&|| AcceptRanges.encode()), "bytes");
    assert_eq!(
        sent(&|| ContentRange::Satisfied {
            first: 42,
            last: 1233,
            complete_length: 1234
        }
        .encode()),
        // Section 14.4's own worked example.
        "bytes 42-1233/1234"
    );
    assert_eq!(
        sent(&|| ContentRange::Unsatisfied {
            complete_length: 47022
        }
        .encode()),
        // Section 15.5.17's own worked example.
        "bytes */47022"
    );
}
