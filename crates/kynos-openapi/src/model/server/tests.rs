use super::{Server, ServerVariable};

#[test]
fn a_bare_server_serializes_to_just_its_url() {
    let json = serde_json::to_string(&Server::new("https://api.example.com")).expect("ok");
    assert_eq!(json, r#"{"url":"https://api.example.com"}"#);
}

#[test]
fn enumerated_variables_carry_their_value_set() {
    let variable = ServerVariable::enumerated("v1", ["v1", "v2"]);
    assert_eq!(variable.default_value, "v1");
    assert_eq!(
        variable.enumeration.as_deref(),
        Some(&["v1".to_owned(), "v2".to_owned()][..])
    );
}
