fn main() {
    let template = kynos::path!("/users/{id}");
    assert_eq!(template.variables(), ["id"]);
}
