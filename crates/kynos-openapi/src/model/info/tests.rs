use super::{Info, License};

#[test]
fn info_serializes_only_the_required_fields_when_bare() {
    let json = serde_json::to_string(&Info::new("Orders", "1.0.0")).expect("serializable");
    assert_eq!(json, r#"{"title":"Orders","version":"1.0.0"}"#);
}

#[test]
fn a_license_serializes_only_the_link_it_carries() {
    let json = serde_json::to_string(&License::spdx("MIT", "MIT")).expect("serializable");
    assert_eq!(json, r#"{"name":"MIT","identifier":"MIT"}"#);
}

#[test]
fn a_license_setting_both_links_does_not_deserialize() {
    let error = serde_json::from_str::<License>(
        r#"{"name":"MIT","identifier":"MIT","url":"https://example.com/LICENSE"}"#,
    )
    .expect_err("`identifier` and `url` are mutually exclusive");

    assert!(
        error.to_string().contains("mutually exclusive"),
        "the rejection should say why: {error}"
    );
}
