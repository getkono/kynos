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
/// Total. The caller supplies the `charset` and `language` halves.
#[must_use]
pub(crate) fn encode_ext_value(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, EXT_VALUE_ENCODE_SET).to_string()
}

/// Percent-decodes one captured value, the inverse of what this module writes
/// into a rendered path.
///
/// Lives here because the dependency table confines `percent-encoding` to this
/// file.
///
/// # Errors
///
/// Returns the error when the decoded bytes are not valid UTF-8.
pub(crate) fn decode_path_value(
    value: &str,
) -> Result<std::borrow::Cow<'_, str>, std::str::Utf8Error> {
    percent_encoding::percent_decode_str(value).decode_utf8()
}

/// The pairs a raw query string carries, each half decoded to octets, in the
/// order the target wrote them.
///
/// Shared by `QueryParams`, query API keys and `Form<T>`, so they cannot
/// disagree about what the client sent; octets, so each reader decides what
/// non-UTF-8 means to it.
///
/// Form rules, as OpenAPI requires of `in: query`: `+` is a space and `%2B` a
/// plus sign. An empty pair is skipped, a pair with no `=` has an empty value,
/// and a malformed escape is kept as the literal `%`.
pub fn query_pairs(
    query: Option<&str>,
) -> impl Iterator<Item = (std::borrow::Cow<'_, [u8]>, std::borrow::Cow<'_, [u8]>)> {
    query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode_form_value(name), decode_form_value(value))
        })
}

/// `+` to a space, then percent-decoding; borrowed when neither changed
/// anything.
fn decode_form_value(raw: &str) -> std::borrow::Cow<'_, [u8]> {
    if raw.contains('+') {
        std::borrow::Cow::Owned(
            percent_encoding::percent_decode_str(&raw.replace('+', " ")).collect(),
        )
    } else {
        percent_encoding::percent_decode_str(raw).into()
    }
}

/// Builds a URI for a generated endpoint without dynamic parameters.
#[must_use]
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
