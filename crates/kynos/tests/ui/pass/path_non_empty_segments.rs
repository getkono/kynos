fn main() {
    let template = kynos::path!("/users/all/{id}");
    assert_eq!(template.normalized(), "/users/all/{}");
}
