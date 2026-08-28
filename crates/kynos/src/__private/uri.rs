//! URI construction for the `uri()` inherent method a route attribute emits,
//! and the percent-encoding the rest of the crate reaches through it.

use crate::{
    extract::params::{path::EncodePath, query::EncodeQuery},
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

/// RFC 8187 `attr-char`: `token` minus `*`, `'` and `%`.
///
/// Written as a subtraction from `NON_ALPHANUMERIC` because the ABNF is itself
/// a subtraction: everything alphanumeric, plus the twelve marks the production
/// lists, and nothing else.
const EXT_VALUE_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

/// Percent-encodes one value as the `value-chars` half of an RFC 8187
/// `ext-value`, which is what a `filename*` parameter carries after
/// `UTF-8''`.
///
/// Total: every input has an encoding, since anything outside `attr-char`
/// becomes UTF-8 octets and then percent triplets. The caller supplies the
/// `charset` and `language` halves, because those are constants at every call
/// site Kynos has.
///
/// Lives here for the same reason the decoder below does.
pub fn encode_ext_value(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, EXT_VALUE_ENCODE_SET).to_string()
}

/// Percent-decodes one captured value, the inverse of what this module writes
/// into a rendered path.
///
/// Lives here because this file is where the dependency table puts
/// `percent-encoding`: an extractor performs the decode, and a response header
/// group performs the encode above, but naming the crate at either would put it
/// under a second path.
///
/// # Errors
///
/// Returns the error when the decoded bytes are not valid UTF-8.
pub fn decode_path_value(value: &str) -> Result<std::borrow::Cow<'_, str>, std::str::Utf8Error> {
    percent_encoding::percent_decode_str(value).decode_utf8()
}

/// Builds a URI for a generated endpoint without dynamic parameters.
pub fn endpoint_uri(template: &str) -> Uri {
    template
        .parse()
        .expect("a route macro only emits a valid URI path")
}

/// Builds a URI for a generated endpoint with path parameters.
pub fn endpoint_uri_with_path<P: EncodePath>(template: &str, path: &P) -> Uri {
    render_endpoint_path(template, path)
        .parse()
        .expect("derived path parameters produce a valid URI")
}

/// Builds a URI for a generated endpoint with query parameters.
pub fn endpoint_uri_with_query<Q: EncodeQuery>(template: &str, query: &Q) -> Uri {
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
pub fn endpoint_uri_with_path_and_query<P: EncodePath, Q: EncodeQuery>(
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

fn render_endpoint_path<P: EncodePath>(template: &str, path: &P) -> String {
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
