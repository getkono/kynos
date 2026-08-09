//! URI construction for the `uri()` inherent method a route attribute emits.

use crate::{
    extract::{PathParams, QueryParams},
    http::Uri,
};

const PATH_SEGMENT_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Builds a URI for a generated endpoint without dynamic parameters.
pub fn endpoint_uri(template: &str) -> Uri {
    template
        .parse()
        .expect("a route macro only emits a valid URI path")
}

/// Builds a URI for a generated endpoint with path parameters.
pub fn endpoint_uri_with_path<P: PathParams>(template: &str, path: &P) -> Uri {
    render_endpoint_path(template, path)
        .parse()
        .expect("derived path parameters produce a valid URI")
}

/// Builds a URI for a generated endpoint with query parameters.
pub fn endpoint_uri_with_query<Q: QueryParams>(template: &str, query: &Q) -> Uri {
    let query = query.encode();
    let uri = if query.is_empty() {
        template.to_owned()
    } else {
        format!("{template}?{query}")
    };
    uri.parse()
        .expect("derived query parameters produce a valid URI")
}

/// Builds a URI for a generated endpoint with path and query parameters.
pub fn endpoint_uri_with_path_and_query<P: PathParams, Q: QueryParams>(
    template: &str,
    path: &P,
    query: &Q,
) -> Uri {
    let path = render_endpoint_path(template, path);
    let query = query.encode();
    let uri = if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    };
    uri.parse()
        .expect("derived endpoint parameters produce a valid URI")
}

fn render_endpoint_path<P: PathParams>(template: &str, path: &P) -> String {
    let values = path.encode();
    assert_eq!(
        values.len(),
        P::NAMES.len(),
        "PathParams::encode must return one value per declared name"
    );

    let mut rendered = template.to_owned();
    for (name, value) in values {
        assert!(
            P::NAMES.contains(&name),
            "PathParams::encode returned undeclared name `{name}`"
        );
        let encoded =
            percent_encoding::utf8_percent_encode(&value, PATH_SEGMENT_ENCODE_SET).to_string();
        rendered = rendered.replace(&format!("{{{name}}}"), &encoded);
    }
    rendered
}
