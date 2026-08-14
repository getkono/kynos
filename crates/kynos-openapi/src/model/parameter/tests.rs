use crate::model::{
    parameter::{Parameter, ParameterIn, header::is_ignored_header_parameter, style::Style},
    schema::{Schema, types::SchemaType},
};

#[test]
fn path_parameters_are_required_on_construction() {
    let parameter = Parameter::path("id", Schema::of_type(SchemaType::String));
    assert_eq!(parameter.required, Some(true));
}

#[test]
fn query_parameters_are_not_required_by_default() {
    let parameter = Parameter::query("page", Schema::of_type(SchemaType::Integer));
    assert_eq!(parameter.required, None);
}

// --- The closed style/location table -------------------------------------
//
// 3.2 §"Style Values" gives the table and says "Combinations not represented
// in this table are not permitted", so the expectations below are transcribed
// from the specification rather than read back off `is_valid_for`. An oracle
// derived from the code under test agrees with it wherever both are wrong.

const ALL_STYLES: &[Style] = &[
    Style::Matrix,
    Style::Label,
    Style::Simple,
    Style::Form,
    Style::SpaceDelimited,
    Style::PipeDelimited,
    Style::DeepObject,
    #[cfg(feature = "openapi32")]
    Style::Cookie,
];

const ALL_LOCATIONS: &[ParameterIn] = &[
    ParameterIn::Query,
    ParameterIn::Header,
    ParameterIn::Path,
    ParameterIn::Cookie,
    #[cfg(feature = "openapi32")]
    ParameterIn::Querystring,
];

/// The `in` column of the specification's table, verbatim.
///
/// The match is exhaustive, so a style added to [`Style`] stops this file
/// compiling until the specification's row for it is written down.
fn permitted_locations(style: Style) -> &'static [ParameterIn] {
    match style {
        Style::Matrix | Style::Label => &[ParameterIn::Path],
        Style::Simple => &[ParameterIn::Path, ParameterIn::Header],
        Style::Form => &[ParameterIn::Query, ParameterIn::Cookie],
        Style::SpaceDelimited | Style::PipeDelimited | Style::DeepObject => &[ParameterIn::Query],
        #[cfg(feature = "openapi32")]
        Style::Cookie => &[ParameterIn::Cookie],
    }
}

#[test]
fn the_style_location_table_is_closed() {
    for &style in ALL_STYLES {
        for &location in ALL_LOCATIONS {
            let permitted = permitted_locations(style).contains(&location);
            assert_eq!(
                style.is_valid_for(location),
                permitted,
                "{style:?} at {location:?}: the specification says {}",
                if permitted {
                    "permitted"
                } else {
                    "not permitted"
                }
            );
        }
    }
}

/// Both lists cover their enum.
///
/// A `const` list cannot be checked for completeness by the compiler, so it is
/// checked against the source — the same instrument `tests/wire.rs` and the
/// `SpecError` ledger use.
#[test]
fn the_style_and_location_lists_are_complete() {
    /// Variants of `name` that are compiled into *this* build.
    ///
    /// The gated ones have to be skipped when their feature is off, or the
    /// count is of the source rather than of the enum the test can see.
    fn variants_of(source: &str, name: &str) -> usize {
        let body = source
            .split_once(&format!("pub enum {name} {{"))
            .expect("the enum is declared in this file")
            .1
            .split_once("\n}")
            .expect("and closed")
            .0;

        let mut count = 0;
        let mut gated = false;
        for line in body.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[cfg(feature = \"openapi32\")]") {
                gated = true;
                continue;
            }
            if trimmed.starts_with(|c: char| c.is_ascii_uppercase()) && trimmed.ends_with(',') {
                if !gated || cfg!(feature = "openapi32") {
                    count += 1;
                }
                gated = false;
            }
        }
        count
    }

    let styles = include_str!("style.rs");
    assert_eq!(variants_of(styles, "Style"), ALL_STYLES.len());
    assert_eq!(
        variants_of(include_str!("mod.rs"), "ParameterIn"),
        ALL_LOCATIONS.len()
    );
}

#[test]
fn style_defaults_follow_the_parameter_location() {
    // The four the specification gives a default for. `querystring` is absent
    // deliberately: `style` "MUST NOT be used with `in: "querystring"`", so
    // there is no default for it to have.
    assert_eq!(Style::default_for(ParameterIn::Query), Style::Form);
    assert_eq!(Style::default_for(ParameterIn::Path), Style::Simple);
    assert_eq!(Style::default_for(ParameterIn::Header), Style::Simple);
    assert_eq!(Style::default_for(ParameterIn::Cookie), Style::Form);
}

#[test]
fn explode_defaults_to_true_for_the_two_styles_that_pair_names_with_values() {
    // "When `style` is `form` or `cookie`, the default value is `true`. For all
    // other styles, the default value is `false`."
    for &style in ALL_STYLES {
        let expected = match style {
            Style::Form => true,
            #[cfg(feature = "openapi32")]
            Style::Cookie => true,
            _ => false,
        };
        assert_eq!(
            style.default_explode(),
            expected,
            "{style:?} should default `explode` to {expected}"
        );
    }
}

#[test]
fn effective_style_and_explode_fall_back_to_defaults() {
    let parameter = Parameter::query("tags", Schema::of_type(SchemaType::Array));
    assert_eq!(parameter.effective_style(), Some(Style::Form));
    assert_eq!(parameter.effective_explode(), Some(true));
}

#[test]
fn a_content_described_parameter_has_no_style_to_report() {
    use crate::model::body::media_type::MediaType;

    let parameter = Parameter::with_content(
        "filter",
        ParameterIn::Query,
        "application/json",
        MediaType::default(),
    );

    // Not "the default style" -- no style at all. `style` is a schema-side
    // field, and a content-described parameter has no place to put one.
    assert_eq!(parameter.style(), None);
    assert_eq!(parameter.effective_style(), None);
    assert_eq!(parameter.effective_explode(), None);
    assert!(parameter.schema().is_none());
    assert_eq!(
        parameter.content().map(|(name, _)| name),
        Some("application/json")
    );
}

#[test]
fn a_parameter_round_trips_through_each_shape() {
    use crate::model::body::media_type::MediaType;

    for parameter in [
        Parameter::query("tags", Schema::of_type(SchemaType::Array)).with_style(Style::Form, true),
        Parameter::with_content(
            "filter",
            ParameterIn::Query,
            "application/json",
            MediaType::default(),
        ),
    ] {
        let json = serde_json::to_string(&parameter).expect("serializable");
        let parsed: Parameter = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(parsed, parameter);
    }
}

#[test]
fn a_parameter_describing_its_value_twice_or_not_at_all_is_refused() {
    let neither = serde_json::from_str::<Parameter>(r#"{"name":"a","in":"query"}"#)
        .expect_err("one of `schema` and `content` is required");
    assert!(neither.to_string().contains("required"));

    let both = serde_json::from_str::<Parameter>(
        r#"{"name":"a","in":"query","schema":true,"content":{"application/json":{}}}"#,
    )
    .expect_err("`schema` and `content` are mutually exclusive");
    assert!(both.to_string().contains("mutually exclusive"));

    let two = serde_json::from_str::<Parameter>(
        r#"{"name":"a","in":"query","content":{"application/json":{},"text/plain":{}}}"#,
    )
    .expect_err("`content` must hold exactly one entry");
    assert!(two.to_string().contains("exactly one entry"));
}

#[test]
fn a_parameter_shown_both_ways_at_once_is_refused() {
    let error = serde_json::from_str::<Parameter>(
        r#"{"name":"a","in":"query","schema":true,"example":1,"examples":{"one":{"value":1}}}"#,
    )
    .expect_err("`example` is exclusive with `examples`");

    assert!(error.to_string().contains("mutually exclusive"));
}

#[test]
fn a_header_declaring_any_other_style_is_refused() {
    use crate::model::parameter::header::Header;

    // The specification gives a header one legal style, so the others are not
    // a combination to report but a word `HeaderStyle` cannot read.
    for style in ["form", "matrix", "label", "spaceDelimited", "deepObject"] {
        let json = format!(r#"{{"schema":true,"style":"{style}"}}"#);
        let error = serde_json::from_str::<Header>(&json)
            .expect_err("`simple` is the only style a header may declare");

        assert!(
            error.to_string().contains("simple"),
            "the error for `{style}` should name the style that is legal, got: {error}"
        );
    }
}

#[test]
fn a_header_round_trips_through_each_form_of_style() {
    use crate::model::parameter::{header::Header, style::HeaderStyle};

    // Stating `simple` and leaving it out are different descriptions of the
    // same serialization, and each survives the trip back out as it arrived.
    let stated =
        Header::new(Schema::of_type(SchemaType::String)).with_style(HeaderStyle::Simple, false);
    assert_eq!(stated.style(), Some(HeaderStyle::Simple));
    assert_eq!(
        serde_json::to_value(&stated).expect("serializable"),
        serde_json::json!({"style": "simple", "explode": false, "schema": {"type": "string"}})
    );

    let absent = Header::new(Schema::of_type(SchemaType::String));
    assert_eq!(absent.style(), None);
    assert_eq!(
        serde_json::to_value(&absent).expect("serializable"),
        serde_json::json!({"schema": {"type": "string"}})
    );

    for header in [stated, absent] {
        let json = serde_json::to_string(&header).expect("serializable");
        let parsed: Header = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(parsed, header);
    }
}

#[test]
fn a_content_described_header_has_no_style_to_set() {
    use crate::model::{
        body::media_type::MediaType,
        parameter::{header::Header, style::HeaderStyle},
    };

    // The same reason a content-described parameter has none: `style` lives in
    // the schema variant, so there is nowhere for this call to write.
    let header = Header::with_content("text/plain", MediaType::default())
        .with_style(HeaderStyle::Simple, true);

    assert_eq!(header.style(), None);
    assert_eq!(
        serde_json::to_value(&header).expect("serializable"),
        serde_json::json!({"content": {"text/plain": {}}})
    );
}

#[test]
fn a_header_shown_both_ways_at_once_is_refused() {
    use crate::model::parameter::header::Header;

    let error = serde_json::from_str::<Header>(
        r#"{"schema":true,"example":1,"examples":{"one":{"value":1}}}"#,
    )
    .expect_err("`example` is exclusive with `examples`");

    assert!(error.to_string().contains("mutually exclusive"));
}

#[test]
fn each_form_of_example_round_trips() {
    use crate::model::{example::Example, parameter::header::Header};

    for parameter in [
        Parameter::query("page", Schema::of_type(SchemaType::Integer)).with_example(1),
        Parameter::query("page", Schema::of_type(SchemaType::Integer))
            .with_named_example("first", Example::new(1)),
    ] {
        let json = serde_json::to_string(&parameter).expect("serializable");
        let parsed: Parameter = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(parsed, parameter);
    }

    for header in [
        Header::new(Schema::of_type(SchemaType::String)).with_example("text/plain"),
        Header::new(Schema::of_type(SchemaType::String))
            .with_named_example("plain", Example::new("text/plain")),
    ] {
        let json = serde_json::to_string(&header).expect("serializable");
        let parsed: Header = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(parsed, header);
    }
}

#[test]
fn the_two_forms_of_example_replace_each_other() {
    use crate::model::example::Example;

    let named = Parameter::query("page", Schema::of_type(SchemaType::Integer))
        .with_example(1)
        .with_named_example("first", Example::new(1));
    assert!(named.example().is_none());
    assert!(named.named_examples().is_some_and(|named| named.len() == 1));

    let inline = Parameter::query("page", Schema::of_type(SchemaType::Integer))
        .with_named_example("first", Example::new(1))
        .with_example(1);
    assert!(inline.named_examples().is_none());
    assert!(inline.example().is_some());
}

#[test]
fn headers_the_spec_ignores_are_recognized_case_insensitively() {
    assert!(is_ignored_header_parameter("Authorization"));
    assert!(is_ignored_header_parameter("content-type"));
    assert!(is_ignored_header_parameter("ACCEPT"));
    assert!(!is_ignored_header_parameter("X-Request-Id"));
}
