use super::Link;

#[test]
fn a_link_pointing_nowhere_or_at_two_operations_is_refused() {
    let neither = serde_json::from_str::<Link>("{}")
        .expect_err("one of `operationRef` and `operationId` is required");
    assert!(neither.to_string().contains("required"));

    let both = serde_json::from_str::<Link>(
        r##"{"operationRef":"#/paths/~1a/get","operationId":"getA"}"##,
    )
    .expect_err("`operationRef` and `operationId` are mutually exclusive");
    assert!(both.to_string().contains("mutually exclusive"));
}
