use super::{header_name_eq, header_names_disjoint, statuses_disjoint};

#[test]
fn header_names_compare_without_regard_to_case() {
    assert!(header_name_eq("X-Request-Id", "x-request-id"));
    assert!(!header_name_eq("x-request-id", "x-request-ids"));
    assert!(!header_name_eq("x-a", "x-b"));
}

#[test]
fn a_shared_header_is_not_disjoint() {
    assert!(header_names_disjoint(&["x-a"], &["x-b", "x-c"]));
    assert!(!header_names_disjoint(&["x-a", "x-b"], &["X-B"]));
    assert!(header_names_disjoint(&[], &["x-a"]));
}

#[test]
fn a_shared_status_is_not_disjoint() {
    assert!(statuses_disjoint(&[413], &[503, 504]));
    assert!(!statuses_disjoint(&[429, 503], &[503]));

    // An interceptor that never short-circuits collides with nothing,
    // which is what lets any number of them compose.
    assert!(statuses_disjoint(&[], &[503]));
}

#[test]
fn the_checks_are_usable_in_const_context() {
    // The whole point: these run while the program is compiled, which is
    // what lets `Router::intercept` reject a collision there rather than
    // when the router is built.
    const {
        assert!(header_names_disjoint(&["x-a"], &["x-b"]));
        assert!(statuses_disjoint(&[413], &[504]));
        assert!(!header_names_disjoint(&["x-a"], &["X-A"]));
    }
}
