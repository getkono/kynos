use kynos_openapi::{Components, Method, Paths, Ref, RefOr, Response};

use super::shared_responses;

/// Shares `response`, written as JSON text, declared at `key` on a `GET` at
/// `/a` and at `/b`, beside `components`. Read from text, so every object,
/// a `serde_json::Value`'s included, keeps the order it is written in under
/// `serde_json/preserve_order`.
fn hoisted(key: &str, response: &str, mut components: Components) -> Paths {
    let operation = format!(r#"{{"get": {{"responses": {{"{key}": {response}}}}}}}"#);
    let mut paths: Paths =
        serde_json::from_str(&format!(r#"{{"/a": {operation}, "/b": {operation}}}"#))
            .expect("paths");
    shared_responses(&mut paths, &mut components);
    paths
}

/// The entry the `GET` at `/a` declares at `key`.
fn entry<'p>(paths: &'p Paths, key: &str) -> &'p RefOr<Response> {
    &paths.items["/a"]
        .operation(Method::Get)
        .expect("a GET")
        .responses
        .responses[key]
}

/// The media type declared first names the response, not the one that sorts
/// first.
#[test]
fn a_response_is_named_for_the_media_type_declared_first() {
    let paths = hoisted(
        "200",
        r##"{
            "description": "the reading",
            "content": {
                "text/plain": {"schema": {"$ref": "#/components/schemas/Note"}},
                "application/json": {"schema": {"$ref": "#/components/schemas/Reading"}}
            }
        }"##,
        Components::default(),
    );

    assert_eq!(
        entry(&paths, "200"),
        &RefOr::Ref(Ref::response("NoteStatus200"))
    );
}

/// A suffix is hashed from a key-sorted form, so neither the order the
/// response's fields serialize in nor the order a nested object's keys were
/// written in, which `serde_json/preserve_order` keeps, changes it. The
/// literal is the one every build must produce.
#[test]
fn a_suffix_is_the_same_whatever_order_an_object_s_keys_are_in() {
    for sample in [r#"{"b": 1, "a": 2}"#, r#"{"a": 2, "b": 1}"#] {
        let mut components = Components::default();
        components
            .responses
            .insert("Status500".to_owned(), RefOr::Item(Response::new("theirs")));

        let paths = hoisted(
            "500",
            &format!(
                r#"{{"description": "failed", "content": {{"text/plain": {{}}}}, "x-sample": {sample}}}"#
            ),
            components,
        );

        assert_eq!(
            entry(&paths, "500"),
            &RefOr::Ref(Ref::response("Status500_71c83d0a")),
            "{sample}"
        );
    }
}
