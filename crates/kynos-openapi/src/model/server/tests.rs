use super::Server;

#[test]
fn a_bare_server_serializes_to_just_its_url() {
    let json = serde_json::to_string(&Server::new("https://api.example.com")).expect("ok");
    assert_eq!(json, r#"{"url":"https://api.example.com"}"#);
}
