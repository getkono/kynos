use super::is_json;

/// A structured syntax suffix is JSON, which is what lets a vendor media
/// type be decoded as the JSON it is.
#[test]
fn a_json_suffixed_media_type_is_read_as_json() {
    assert!(is_json("application/json"));
    assert!(is_json("application/json; charset=utf-8"));
    assert!(is_json("APPLICATION/JSON"));
    assert!(is_json("application/vnd.acme.filter+json"));

    assert!(!is_json("application/xml"));
    assert!(!is_json("text/plain"));
    // A suffix is a suffix of the base type, not of the parameters.
    assert!(!is_json("application/xml; note=+json"));
}
