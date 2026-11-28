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

/// Responses differing only in the order an object's keys were written in,
/// which `serde_json/preserve_order` keeps in a nested `Value`, are one
/// response in every build, so the canonical description does not change
/// with that feature.
#[test]
fn responses_differing_only_in_key_order_share_one_component() {
    let response = |sample: &str| {
        format!(
            r#"{{"description": "failed", "content": {{"text/plain": {{}}}}, "x-sample": {sample}}}"#
        )
    };
    let operation = |sample: &str| {
        format!(
            r#"{{"get": {{"responses": {{"500": {}}}}}}}"#,
            response(sample)
        )
    };
    let (sorted, reversed) = (
        operation(r#"{"a": 2, "b": 1}"#),
        operation(r#"{"b": 1, "a": 2}"#),
    );
    let mut paths: Paths = serde_json::from_str(&format!(
        r#"{{"/a": {sorted}, "/b": {sorted}, "/c": {reversed}, "/d": {reversed}}}"#
    ))
    .expect("paths");
    let mut components = Components::default();

    shared_responses(&mut paths, &mut components);

    for path in ["/a", "/b", "/c", "/d"] {
        assert_eq!(
            paths.items[path]
                .operation(Method::Get)
                .expect("a GET")
                .responses
                .responses["500"],
            RefOr::Ref(Ref::response("Status500")),
            "{path}"
        );
    }
    assert_eq!(components.responses.len(), 1);
}

/// A response's own media types, headers and links keep their declared
/// order: negotiation sends the first media type offered, so a response
/// offering JSON first and one offering text first say different things,
/// and stay two components in every build, each listing its own order. Both
/// derive `Status200`, so each takes a suffix; the literals are the ones
/// every build must produce.
#[test]
fn responses_differing_in_media_type_order_stay_apart() {
    let content = |first: &str, second: &str| {
        format!(
            r#"{{"get": {{"responses": {{"200": {{"description": "OK", "content": {{"{first}": {{}}, "{second}": {{}}}}}}}}}}}}"#
        )
    };
    let (json, text) = (
        content("application/json", "text/plain"),
        content("text/plain", "application/json"),
    );
    let mut paths: Paths = serde_json::from_str(&format!(
        r#"{{"/a": {json}, "/b": {json}, "/c": {text}, "/d": {text}}}"#
    ))
    .expect("paths");
    let mut components = Components::default();

    shared_responses(&mut paths, &mut components);

    for (operations, name, first) in [
        (["/a", "/b"], "Status200_676f0c5e", "application/json"),
        (["/c", "/d"], "Status200_a5eb568b", "text/plain"),
    ] {
        for path in operations {
            assert_eq!(
                paths.items[path]
                    .operation(Method::Get)
                    .expect("a GET")
                    .responses
                    .responses["200"],
                RefOr::Ref(Ref::response(name)),
                "{path}"
            );
        }
        let component = components.responses[name].as_item().expect("inline");
        assert_eq!(
            component.content.keys().next().map(String::as_str),
            Some(first),
            "{name}"
        );
    }
    assert_eq!(components.responses.len(), 2);
}

/// Shares a `200` response whose `field` declares the entries `first` and
/// `second` give, in that order, on a `GET` at `/a` and `/b`, and one
/// declaring them the other way round at `/c` and `/d`, checks the first two
/// now reference the component `names[0]` and the last two `names[1]`, the
/// only two components, and returns those components.
fn shared_in_both_orders(field: &str, first: &str, second: &str, names: [&str; 2]) -> Components {
    let operation = |one: &str, two: &str| {
        format!(
            r#"{{"get": {{"responses": {{"200": {{"description": "OK", "{field}": {{{one}, {two}}}}}}}}}}}"#
        )
    };
    let (declared, reversed) = (operation(first, second), operation(second, first));
    let mut paths: Paths = serde_json::from_str(&format!(
        r#"{{"/a": {declared}, "/b": {declared}, "/c": {reversed}, "/d": {reversed}}}"#
    ))
    .expect("paths");
    let mut components = Components::default();

    shared_responses(&mut paths, &mut components);

    for (operations, name) in [["/a", "/b"], ["/c", "/d"]].into_iter().zip(names) {
        for path in operations {
            assert_eq!(
                paths.items[path]
                    .operation(Method::Get)
                    .expect("a GET")
                    .responses
                    .responses["200"],
                RefOr::Ref(Ref::response(name)),
                "{path}"
            );
        }
    }
    assert_eq!(components.responses.len(), 2);
    components
}

/// Responses differing only in the order their headers are declared in stay
/// two components in every build, each listing its own order, as
/// `responses_differing_in_media_type_order_stay_apart` pins for media
/// types. The literals are the ones every build must produce.
#[test]
fn responses_differing_in_header_order_stay_apart() {
    let names = ["Status200_aa8800bb", "Status200_1049372f"];

    let components = shared_in_both_orders(
        "headers",
        r#""X-Rate-Limit": {"schema": {"type": "integer"}}"#,
        r#""ETag": {"schema": {"type": "string"}}"#,
        names,
    );

    for (name, first) in names.into_iter().zip(["X-Rate-Limit", "ETag"]) {
        let component = components.responses[name].as_item().expect("inline");
        assert_eq!(
            component.headers.keys().next().map(String::as_str),
            Some(first),
            "{name}"
        );
    }
}

/// Responses differing only in the order their links are declared in stay
/// two components in every build, each listing its own order. The literals
/// are the ones every build must produce.
#[test]
fn responses_differing_in_link_order_stay_apart() {
    let names = ["Status200_c5b071ca", "Status200_a5ebdcc0"];

    let components = shared_in_both_orders(
        "links",
        r#""self": {"operationId": "getReading"}"#,
        r#""next": {"operationId": "listReadings"}"#,
        names,
    );

    for (name, first) in names.into_iter().zip(["self", "next"]) {
        let component = components.responses[name].as_item().expect("inline");
        assert_eq!(
            component.links.keys().next().map(String::as_str),
            Some(first),
            "{name}"
        );
    }
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
