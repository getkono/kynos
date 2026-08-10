fn main() {
    let template = kynos::path!("/users/{id}");
    assert_eq!(template.as_str(), "/users/{id}");
}
