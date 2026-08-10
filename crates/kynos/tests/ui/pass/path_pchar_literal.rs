fn main() {
    let template = kynos::path!("/users/a%20b");
    assert_eq!(template.as_str(), "/users/a%20b");
}
