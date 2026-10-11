//! Which mounted operation a link resolves to, and which it refuses.

use kynos_openapi::{Method, PathTemplate};

use super::{Target, UnresolvedLink};

/// What `#[kynos::get("/{id}")] async fn get_user(..)` declares.
const GET_USER: Target = Target {
    operation_id: "get_user",
    method: "GET",
    path: "/{id}",
    variables: &["id"],
};

fn template(path: &str) -> PathTemplate {
    PathTemplate::parse(path).expect("a valid template")
}

fn unmounted() -> UnresolvedLink {
    UnresolvedLink::Unmounted {
        from: "create_user".to_owned(),
        target: "get_user",
    }
}

#[test]
fn a_target_under_a_constant_prefix_resolves() {
    let path = template("/v1/users/{id}");

    assert_eq!(
        GET_USER.resolve("create_user", Some((Method::Get, &path))),
        Ok(())
    );
}

#[test]
fn a_target_nothing_mounts_is_refused() {
    assert_eq!(GET_USER.resolve("create_user", None), Err(unmounted()));
}

/// The `operationId` is the lookup key, so another operation holding it -- a
/// builder's, with the attribute's handler left unmounted -- is not the target.
#[test]
fn another_operation_holding_the_id_is_not_the_target() {
    let other_method = template("/users/{id}");
    let other_path = template("/users/{id}/avatar");

    assert_eq!(
        GET_USER.resolve("create_user", Some((Method::Delete, &other_method))),
        Err(unmounted())
    );
    assert_eq!(
        GET_USER.resolve("create_user", Some((Method::Get, &other_path))),
        Err(unmounted())
    );
}

/// Nothing `get_user` extracts names `tenant`, so its URI could not be
/// rendered.
#[test]
fn a_prefix_variable_is_refused() {
    let path = template("/tenants/{tenant}/users/{id}");

    assert_eq!(
        GET_USER.resolve("create_user", Some((Method::Get, &path))),
        Err(UnresolvedLink::PrefixVariable {
            from: "create_user".to_owned(),
            target: "get_user",
            path: "/tenants/{tenant}/users/{id}".to_owned(),
        })
    );
}
