use super::Link;

#[test]
fn a_link_pointing_nowhere_or_at_two_operations_is_refused() {
    let neither = serde_json::from_value::<Link>(serde_json::json!({}))
        .expect_err("one of `operationRef` and `operationId` is required");
    assert_eq!(
        neither.to_string(),
        "one of `operationRef` and `operationId` is required on a Link Object"
    );

    let both = serde_json::from_value::<Link>(serde_json::json!({
        "operationRef": "#/paths/~1a/get",
        "operationId": "getA",
    }))
    .expect_err("`operationRef` and `operationId` are mutually exclusive");
    assert_eq!(
        both.to_string(),
        "`operationRef` and `operationId` are mutually exclusive on a Link Object"
    );
}
