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

#[test]
fn style_defaults_follow_the_parameter_location() {
    assert_eq!(Style::default_for(ParameterIn::Query), Style::Form);
    assert_eq!(Style::default_for(ParameterIn::Path), Style::Simple);
    assert_eq!(Style::default_for(ParameterIn::Header), Style::Simple);
    assert_eq!(Style::default_for(ParameterIn::Cookie), Style::Form);
}

#[test]
fn the_style_location_table_is_closed() {
    assert!(Style::Matrix.is_valid_for(ParameterIn::Path));
    assert!(!Style::Matrix.is_valid_for(ParameterIn::Query));
    assert!(Style::DeepObject.is_valid_for(ParameterIn::Query));
    assert!(!Style::DeepObject.is_valid_for(ParameterIn::Path));
    assert!(!Style::Form.is_valid_for(ParameterIn::Header));
}

#[test]
fn explode_defaults_to_true_only_for_form() {
    assert!(Style::Form.default_explode());
    assert!(!Style::Simple.default_explode());
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
