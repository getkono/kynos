use crate::{
    __private::{path::path_parameter_names_match, uri::endpoint_uri_with_path},
    extract::PathParams,
};

struct Params;

impl PathParams for Params {
    const NAMES: &'static [&'static str] = &["name"];

    fn encode(&self) -> Vec<(&'static str, String)> {
        vec![("name", "sales/2026 report".to_owned())]
    }
}

#[test]
fn typed_endpoint_paths_percent_encode_each_segment() {
    let uri = endpoint_uri_with_path("/reports/{name}", &Params);
    assert_eq!(uri, "/reports/sales%2F2026%20report");
}

#[test]
fn path_parameter_names_compare_in_const_context() {
    const MATCHES: bool = path_parameter_names_match(&["tenant", "id"], &["tenant", "id"]);
    const DIFFERS: bool = path_parameter_names_match(&["tenant", "id"], &["id", "tenant"]);
    assert!(std::hint::black_box(MATCHES));
    assert!(!std::hint::black_box(DIFFERS));
}
