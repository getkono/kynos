fn main() {
    let template = kynos::path!("/users/{name}");
    assert_eq!(template.variables(), ["name"]);
}
