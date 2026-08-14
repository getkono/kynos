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
fn a_path_segment_is_never_empty() {
    // `path-segment = 1*( path-literal / template-expression )`, so a segment
    // always holds something.
    for raw in ["//", "//users", "/users//posts", "/users//"] {
        assert!(
            matches!(
                PathTemplate::parse(raw),
                Err(InvalidPathTemplate::EmptySegment(_))
            ),
            "`{raw}` should not parse"
        );
    }
}

#[test]
fn the_root_and_a_trailing_slash_are_still_paths() {
    // The final segment is optional in the grammar, so `/` and `/users/` are
    // both well formed — and they are different paths, which is the point of
    // the trailing-slash policy being an application-level decision.
    assert_eq!(PathTemplate::parse("/").expect("valid").as_str(), "/");
    assert_eq!(
        PathTemplate::parse("/users/").expect("valid").as_str(),
        "/users/"
    );
    assert_eq!(
        PathTemplate::parse("/users/{id}/").expect("valid").as_str(),
        "/users/{id}/"
    );
}

/// A variable name may hold a `/`, so segmenting cannot simply split on one.
#[test]
fn a_slash_inside_an_expression_does_not_open_a_segment() {
    let template = PathTemplate::parse("/files/{a/b}").expect("valid");
    assert_eq!(template.variables(), ["a/b"]);
}

#[test]
fn a_stray_closing_brace_reads_as_unbalanced_wherever_it_is() {
    // Before the first expression and after the last are the same mistake, so
    // they must not produce two different diagnostics.
    assert!(matches!(
        PathTemplate::parse("/a}/b/{id}"),
        Err(InvalidPathTemplate::UnbalancedBraces(_))
    ));
    assert!(matches!(
        PathTemplate::parse("/{id}/a}"),
        Err(InvalidPathTemplate::UnbalancedBraces(_))
    ));
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
fn the_method_list_is_complete() {
    assert_eq!(Method::all(), EVERY_METHOD);
}

/// Every method with a dedicated Path Item field, transcribed rather than read
/// from [`Method::all`], so a variant dropped from that list fails here.
const EVERY_METHOD: &[Method] = &[
    Method::Get,
    Method::Put,
    Method::Post,
    Method::Delete,
    Method::Options,
    Method::Head,
    Method::Patch,
    Method::Trace,
    #[cfg(feature = "openapi32")]
    Method::Query,
];

/// A method's two spellings: the HTTP token, and the Path Item field name.
///
/// They are different mappings — `as_wire_str` is uppercase and serde's
/// `rename_all = "lowercase"` is not — and each was previously checked against
/// itself. `from_wire_str` was compared to `as_wire_str`, which is one table
/// read twice, so two spellings swapped between variants agreed in both
/// directions; and of the nine field names, one was pinned.
///
/// An exhaustive match, so a method added to [`Method`] stops this file
/// compiling until both of its spellings are written down.
fn spellings(method: Method) -> (&'static str, &'static str) {
    match method {
        Method::Get => ("GET", "get"),
        Method::Put => ("PUT", "put"),
        Method::Post => ("POST", "post"),
        Method::Delete => ("DELETE", "delete"),
        Method::Options => ("OPTIONS", "options"),
        Method::Head => ("HEAD", "head"),
        Method::Patch => ("PATCH", "patch"),
        Method::Trace => ("TRACE", "trace"),
        #[cfg(feature = "openapi32")]
        Method::Query => ("QUERY", "query"),
    }
}

#[test]
fn every_method_carries_the_two_spellings_the_specification_gives_it() {
    for &method in EVERY_METHOD {
        let (token, field) = spellings(method);

        assert_eq!(method.as_wire_str(), token, "{method:?} as a method token");
        assert_eq!(method.to_string(), token, "{method:?} rendered");
        assert_eq!(
            Method::from_wire_str(token),
            Some(method),
            "{token} parsed back"
        );

        assert_eq!(
            serde_json::to_value(method).expect("a method is representable"),
            serde_json::Value::String(field.to_owned()),
            "{method:?} as a Path Item field"
        );
        assert_eq!(
            serde_json::from_value::<Method>(serde_json::Value::String(field.to_owned()))
                .expect("a field name the model writes, the model reads"),
            method,
            "{field} read back"
        );
    }
}

#[test]
fn wire_spellings_are_case_sensitive_and_closed() {
    assert_eq!(Method::from_wire_str("get"), None);
    assert_eq!(Method::from_wire_str("PROPFIND"), None);
    assert_eq!(Method::from_wire_str(""), None);
}
