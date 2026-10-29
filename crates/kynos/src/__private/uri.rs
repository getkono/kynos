//! URI construction for the `EndpointUri` implementation a route attribute
//! emits, and the percent-encoding the rest of the crate reaches through it.

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
/// Shared by `QueryParams` and query API keys, so they cannot disagree about
/// what the client sent; octets, so each reader decides what non-UTF-8 means
/// to it. `Form<T>` does not read its body through this: it checks the body
/// in place with `check_percent_decoded_utf8`, which the `__private` test
/// `the_in_place_utf8_check_agrees_with_decoding_every_pair` and the `query`
/// fuzz target hold to this reader plus `str::from_utf8` on every half.
///
/// Form rules, as OpenAPI requires of `in: query`: `+` is a space and `%2B` a
/// plus sign. An empty pair is skipped, a pair with no `=` has an empty value,
/// and a malformed escape is kept as the literal `%`.
pub(crate) fn query_pairs(
    query: Option<&str>,
) -> impl Iterator<Item = (std::borrow::Cow<'_, [u8]>, std::borrow::Cow<'_, [u8]>)> {
    query_names(query).map(|(name, value)| (name, decode_query_component(value)))
}

/// [`query_pairs`] with each value left as the target wrote it, for a reader
/// that decodes only the values it reads, through [`decode_query_component`].
pub fn query_names(
    query: Option<&str>,
) -> impl Iterator<Item = (std::borrow::Cow<'_, [u8]>, &str)> {
    query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode_query_component(name), value)
        })
}

/// Whether `text` percent-decodes to UTF-8, read in place without decoding it
/// into memory.
///
/// `+`, `&` and `=` are ASCII, so this is also whether every half of every
/// pair [`query_pairs`] reads from `text` is UTF-8. A malformed escape is the
/// literal `%`, as there.
///
/// # Errors
///
/// Returns the offset into `text` of the first escape that begins, or
/// continues, an octet sequence that is not UTF-8.
#[cfg(feature = "form")]
pub(crate) fn check_percent_decoded_utf8(text: &str) -> Result<(), usize> {
    let octets = text.as_bytes();
    // The escaped octets of the character being read, and where it began. A
    // literal octet always begins a character of its own, because `text` is a
    // `str`, so only escapes ever need holding.
    let mut pending = [0u8; 4];
    let mut held = 0;
    let mut began = 0;
    let mut index = 0;

    while index < octets.len() {
        if held == 0 {
            // Holding nothing, the literal text before the next `%` is UTF-8
            // already. `index` only ever steps past `%` and escapes, so it is
            // always on a character boundary.
            let Some(skipped) = text[index..].find('%') else {
                break;
            };
            index += skipped;
        }
        let Some(octet) = escaped_octet(octets, index) else {
            if held > 0 {
                return Err(began);
            }
            // A malformed escape: its `%` is literal.
            index += 1;
            continue;
        };

        if held == 0 {
            if octet.is_ascii() {
                index += 3;
                continue;
            }
            began = index;
        }
        pending[held] = octet;
        held += 1;
        match std::str::from_utf8(&pending[..held]) {
            Ok(_) => held = 0,
            // Incomplete, and short of the four octets the longest takes.
            Err(error) if error.error_len().is_none() && held < pending.len() => {}
            Err(_) => return Err(began),
        }
        index += 3;
    }

    if held > 0 { Err(began) } else { Ok(()) }
}

/// The octet the escape at `index` encodes, or `None` where none begins there.
#[cfg(feature = "form")]
fn escaped_octet(octets: &[u8], index: usize) -> Option<u8> {
    let [b'%', high, low, ..] = octets.get(index..)? else {
        return None;
    };
    let digit = |octet: u8| char::from(octet).to_digit(16);
    let octet = digit(*high)? << 4 | digit(*low)?;
    u8::try_from(octet).ok()
}

/// One name or value of a query pair, decoded as [`query_pairs`] decodes it:
/// `+` to a space, then percent-decoding; borrowed when neither changed
/// anything.
#[must_use]
pub fn decode_query_component(raw: &str) -> std::borrow::Cow<'_, [u8]> {
    if raw.contains('+') {
        std::borrow::Cow::Owned(
            percent_encoding::percent_decode_str(&raw.replace('+', " ")).collect(),
        )
    } else {
        percent_encoding::percent_decode_str(raw).into()
    }
}

/// One name or value of a query pair, encoded for [`decode_query_component`]
/// to read back: every octet outside RFC 3986's `unreserved` set becomes an
/// uppercase percent triplet, so a space and a literal `+` both survive the
/// form decoding; borrowed when nothing needed escaping.
#[must_use]
pub fn encode_query_component(raw: &str) -> std::borrow::Cow<'_, str> {
    percent_encoding::utf8_percent_encode(raw, QUERY_COMPONENT_ENCODE_SET).into()
}

/// Everything but RFC 3986 `unreserved`: ALPHA, DIGIT, `-`, `.`, `_`, `~`.
const QUERY_COMPONENT_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Builds a URI for a generated endpoint without dynamic parameters.
#[must_use]
pub fn endpoint_uri(template: &str) -> Uri {
    template
        .parse()
        .expect("a route macro only emits a valid URI path")
}

/// Builds a URI for a generated endpoint with path parameters.
///
/// # Panics
///
/// Panics when a value cannot address the route: an empty one, which no
/// variable captures, or one that renders its segment, with any literal text
/// sharing it, as `.` or `..`, which URI resolution removes (RFC 3986 §5.2.4)
/// even when written `%2E`.
pub fn endpoint_uri_with_path<P: EncodePath>(template: &str, path: &P) -> Uri {
    render_endpoint_path(template, path)
        .parse()
        .expect("derived path parameters produce a valid URI")
}

/// Builds a URI for a generated endpoint with query parameters.
pub fn endpoint_uri_with_query<Q: EncodeQuery>(template: &str, query: &Q) -> Uri {
    with_query(
        template.to_owned(),
        query,
        "derived query parameters produce a valid URI",
    )
}

/// Builds a URI for a generated endpoint with path and query parameters.
///
/// # Panics
///
/// As [`endpoint_uri_with_path`].
pub fn endpoint_uri_with_path_and_query<P: EncodePath, Q: EncodeQuery>(
    template: &str,
    path: &P,
    query: &Q,
) -> Uri {
    with_query(
        render_endpoint_path(template, path),
        query,
        "derived endpoint parameters produce a valid URI",
    )
}

/// `path` with `?` and the encoded `query` appended where there is one, parsed;
/// a failure panics with `expectation`.
fn with_query<Q: EncodeQuery>(mut path: String, query: &Q, expectation: &str) -> Uri {
    let query = query.encode();
    if !query.is_empty() {
        path.push('?');
        path.push_str(&query);
    }
    path.parse().expect(expectation)
}

fn render_endpoint_path<P: EncodePath>(template: &str, path: &P) -> String {
    let values = path.encode();
    assert_eq!(
        values.len(),
        P::NAMES.len(),
        "PathParams::encode must return one value per declared name"
    );

    let mut rendered = template.to_owned();
    for (name, value) in &values {
        assert!(
            P::NAMES.contains(name),
            "PathParams::encode returned undeclared name `{name}`"
        );
        assert!(
            !value.is_empty(),
            "path parameter `{name}` is empty, so no URI renders it into `{template}`"
        );
        let encoded =
            percent_encoding::utf8_percent_encode(value, PATH_SEGMENT_ENCODE_SET).to_string();
        rendered = rendered.replace(&format!("{{{name}}}"), &encoded);
    }
    if let Some((segment, dots)) = dot_segment(template, &rendered) {
        let (name, value) = values
            .iter()
            .find(|(name, _)| segment.contains(&format!("{{{name}}}")))
            .expect("only a segment holding a variable is checked");
        panic!(
            "path parameter `{name}` is `{value}`, which renders `{segment}` as `{dots}`, so no URI \
             renders it into `{template}`"
        );
    }
    rendered
}

/// The first template segment holding a variable that `rendered` turns into a
/// dot segment, with that dot segment.
///
/// Compared after decoding, because resolution (RFC 3986 §5.2.4) decodes `%2E`
/// first, so neither an encoded value nor an encoded literal escapes it. A
/// rendered value holds no `/`, so the two paths split into the same segments.
fn dot_segment<'t>(template: &'t str, rendered: &str) -> Option<(&'t str, &'static str)> {
    template
        .split('/')
        .zip(rendered.split('/'))
        .find(|(segment, rendered)| segment.contains('{') && is_dot_segment(rendered))
        .map(|(segment, rendered)| {
            // A dot segment decodes to one byte or two.
            let dots = match percent_encoding::percent_decode_str(rendered).count() {
                1 => ".",
                _ => "..",
            };
            (segment, dots)
        })
}

/// `.` or `..`, literal or `%2e` in either case: what resolution (RFC 3986
/// §5.2.4) removes once it decodes the segment.
pub(crate) fn is_dot_segment(segment: &str) -> bool {
    let mut rest = segment;
    let mut dots = 0;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            rest = after;
        } else if rest
            .as_bytes()
            .get(..3)
            .is_some_and(|head| head.eq_ignore_ascii_case(b"%2e"))
        {
            rest = &rest[3..];
        } else {
            return false;
        }
        dots += 1;
    }
    matches!(dots, 1 | 2)
}
