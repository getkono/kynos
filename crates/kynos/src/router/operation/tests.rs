//! How a case policy spells a derived `operationId`.

use super::OperationIdCase;

/// Each case joins the words `_` separates, keeping every letter but a word's
/// first as written, and leaves the identifier alone as written.
#[test]
fn each_case_joins_the_words_an_underscore_separates() {
    let cases: [(&str, [&str; 3]); 6] = [
        ("get_user", ["get_user", "getUser", "GetUser"]),
        ("head_file", ["head_file", "headFile", "HeadFile"]),
        ("list", ["list", "list", "List"]),
        ("get_v2_items", ["get_v2_items", "getV2Items", "GetV2Items"]),
        // The rest of a word is not lowered: `HTTP` was written so.
        (
            "get_HTTP_status",
            ["get_HTTP_status", "getHTTPStatus", "GetHTTPStatus"],
        ),
        // An asset's identifier, from a prefix and a path.
        (
            "asset_css_main_css",
            ["asset_css_main_css", "assetCssMainCss", "AssetCssMainCss"],
        ),
    ];
    for (id, [as_written, lower, upper]) in cases {
        assert_eq!(OperationIdCase::AsWritten.apply(id), as_written, "{id}");
        assert_eq!(OperationIdCase::LowerCamel.apply(id), lower, "{id}");
        assert_eq!(OperationIdCase::UpperCamel.apply(id), upper, "{id}");
    }
}

/// An underscore at either end or doubled separates no word, and one that is
/// all underscores keeps its spelling rather than becoming empty, which no
/// `operationId` may be.
#[test]
fn an_underscore_that_separates_no_word_is_dropped() {
    assert_eq!(OperationIdCase::LowerCamel.apply("_get__user_"), "getUser");
    assert_eq!(OperationIdCase::UpperCamel.apply("Get_user"), "GetUser");
    assert_eq!(OperationIdCase::LowerCamel.apply("Get_user"), "getUser");
    assert_eq!(OperationIdCase::LowerCamel.apply("__"), "__");
}
