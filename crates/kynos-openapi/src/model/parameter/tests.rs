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
    assert_eq!(parameter.effective_style(), Style::Form);
    assert!(parameter.effective_explode());
}

#[test]
fn headers_the_spec_ignores_are_recognized_case_insensitively() {
    assert!(is_ignored_header_parameter("Authorization"));
    assert!(is_ignored_header_parameter("content-type"));
    assert!(is_ignored_header_parameter("ACCEPT"));
    assert!(!is_ignored_header_parameter("X-Request-Id"));
}
