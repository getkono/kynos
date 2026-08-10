use crate::model::paths::{
    item::PathItem,
    method::Method,
    operation::Operation,
    template::{InvalidPathTemplate, PathTemplate},
};

#[test]
fn a_template_exposes_its_variables_in_order() {
    let template = PathTemplate::parse("/users/{id}/posts/{postId}").expect("valid");
    assert_eq!(template.variables(), ["id", "postId"]);
}

#[test]
fn a_template_without_variables_is_valid() {
    let template = PathTemplate::parse("/health").expect("valid");
    assert!(template.variables().is_empty());
    assert_eq!(template.normalized(), "/health");
}

#[test]
fn templates_must_begin_with_a_slash() {
    assert!(matches!(
        PathTemplate::parse("users/{id}"),
        Err(InvalidPathTemplate::MissingLeadingSlash(_))
    ));
}

#[test]
fn braces_must_balance() {
    assert!(matches!(
        PathTemplate::parse("/users/{id"),
        Err(InvalidPathTemplate::UnbalancedBraces(_))
    ));
    assert!(matches!(
        PathTemplate::parse("/users/id}"),
        Err(InvalidPathTemplate::UnbalancedBraces(_))
    ));
    assert!(matches!(
        PathTemplate::parse("/users/{{id}"),
        Err(InvalidPathTemplate::UnbalancedBraces(_))
    ));
}

#[test]
fn an_empty_expression_is_rejected() {
    assert!(matches!(
        PathTemplate::parse("/users/{}"),
        Err(InvalidPathTemplate::EmptyExpression(_))
    ));
}

#[test]
fn a_variable_may_not_repeat_within_one_template() {
    assert!(matches!(
        PathTemplate::parse("/a/{id}/b/{id}"),
        Err(InvalidPathTemplate::DuplicateVariable { .. })
    ));
}

#[test]
fn query_strings_and_fragments_are_not_paths() {
    assert!(matches!(
        PathTemplate::parse("/users?page=1"),
        Err(InvalidPathTemplate::NotAPath(_))
    ));
    assert!(matches!(
        PathTemplate::parse("/users#top"),
        Err(InvalidPathTemplate::NotAPath(_))
    ));
}

#[test]
fn literal_segments_accept_every_character_the_grammar_allows() {
    // `unreserved`, `sub-delims`, `:` and `@` are all `pchar`.
    let template =
        PathTemplate::parse("/a-b.c_d~e/f!g$h&i'j(k)l*m+n,o;p=q/r:s@t/%2Fu").expect("valid");
    assert!(template.variables().is_empty());
}

#[test]
fn literal_segments_reject_characters_outside_pchar() {
    for raw in [
        "/users/a b",
        "/users/a<b",
        "/users/a>b",
        "/users/a\"b",
        "/users/a\\b",
        "/users/a^b",
        "/users/a`b",
        "/users/a|b",
        // `pchar` is ASCII; anything else must arrive percent-encoded.
        "/café",
    ] {
        assert!(
            matches!(
                PathTemplate::parse(raw),
                Err(InvalidPathTemplate::IllegalLiteralCharacter { .. })
            ),
            "`{raw}` should not parse"
        );
    }
}

#[test]
fn a_percent_must_introduce_an_encoded_triple() {
    for raw in ["/users/%", "/users/%2", "/users/%zz", "/users/%2z"] {
        assert!(
            matches!(
                PathTemplate::parse(raw),
                Err(InvalidPathTemplate::MalformedPercentEncoding(_))
            ),
            "`{raw}` should not parse"
        );
    }
}

#[test]
fn a_variable_name_may_hold_anything_but_a_brace() {
    // The grammar's `template-expression-param-name` is every Unicode
    // character except `{` and `}`, so the model stays permissive. Kynos's
    // narrower routing contract is enforced above this type, not inside it.
    let template = PathTemplate::parse("/assets/{*path}").expect("valid");
    assert_eq!(template.variables(), ["*path"]);

    let spaced = PathTemplate::parse("/users/{user id}").expect("valid");
    assert_eq!(spaced.variables(), ["user id"]);
}

#[test]
fn templates_differing_only_in_variable_name_normalize_alike() {
    let left = PathTemplate::parse("/pets/{petId}").expect("valid");
    let right = PathTemplate::parse("/pets/{name}").expect("valid");
    assert_ne!(left, right);
    assert_eq!(left.normalized(), right.normalized());
}

#[test]
fn prefixing_concatenates_and_revalidates() {
    let template = PathTemplate::parse("/users/{id}").expect("valid");
    let prefixed = template.with_prefix("/v1").expect("valid");
    assert_eq!(prefixed.as_str(), "/v1/users/{id}");

    // The prefix reintroduces `id`, which the combined template forbids.
    assert!(matches!(
        template.with_prefix("/tenants/{id}"),
        Err(InvalidPathTemplate::DuplicateVariable { .. })
    ));
}

#[test]
fn a_trailing_slash_on_the_prefix_does_not_double_up() {
    let template = PathTemplate::parse("/users").expect("valid");
    assert_eq!(
        template.with_prefix("/v1/").expect("valid").as_str(),
        "/v1/users"
    );
}

#[test]
fn operations_are_addressed_by_method() {
    let item = PathItem::new().with_operation(Method::Get, Operation::new("listUsers"));
    assert!(item.operation(Method::Get).is_some());
    assert!(item.operation(Method::Post).is_none());
    assert_eq!(item.operations().count(), 1);
}

#[test]
fn methods_render_in_wire_case() {
    assert_eq!(Method::Get.to_string(), "GET");
    assert_eq!(Method::Delete.as_wire_str(), "DELETE");
}
