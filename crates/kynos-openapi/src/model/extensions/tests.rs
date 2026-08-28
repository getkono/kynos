use super::Extensions;

#[test]
fn extension_names_require_the_x_prefix() {
    assert!(Extensions::is_valid_name("x-internal-id"));
    assert!(!Extensions::is_valid_name("internal-id"));
}

#[test]
fn oai_reserved_prefixes_are_rejected() {
    assert!(!Extensions::is_valid_name("x-oai-anything"));
    assert!(!Extensions::is_valid_name("x-oas-anything"));
}
