fn main() {
    let template = kynos::path!("/users");
    assert!(template.variables().is_empty());
}
