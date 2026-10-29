//! The HTTP types Kynos builds on.
//!
//! Kynos uses the `http` crate's request and response types, so an application
//! composes with the rest of the ecosystem. No extractor yields a whole
//! [`Request`]: a handler that reads an arbitrary part of the request cannot
//! describe what it read.
//!
//! [`body`] holds the one type Kynos defines; [`cookie`] and [`etag`] read
//! their fields' grammar; [`forwarded`] resolves the client behind trusted
//! proxies; [`media`] names media types in the type system.

pub mod body;
pub mod cookie;
pub mod etag;
pub mod forwarded;
pub mod media;

// Private: a handler only ever sees a coding already chosen.
#[cfg(any(feature = "compression", feature = "assets"))]
pub(crate) mod coding;
// Private: a handler passes a `SystemTime`; only responses render and read dates.
pub(crate) mod date;
// Private: a weight reaches a handler already folded into the winning choice.
pub(crate) mod quality;

use crate::http::body::Body;

#[doc(no_inline)]
pub use http::{
    Extensions, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version, header,
};

/// An incoming request.
pub type Request = http::Request<Body>;

/// The head of an incoming request: everything but the body.
///
/// This is what a [`FromRequestParts`](crate::extract::FromRequestParts)
/// implementation sees, and what a
/// [`FromRequest`](crate::extract::FromRequest) one reads beside the body.
pub type Parts = http::request::Parts;

/// An outgoing response.
pub type Response = http::Response<Body>;

/// Whether `bytes` is a non-empty RFC 9110 section 5.6.2 `token`: visible
/// octets, none of them a delimiter; usable in a constant.
pub(crate) const fn is_token(bytes: &[u8]) -> bool {
    const DELIMITERS: &[u8] = b"\"(),/:;<=>?@[\\]{}";

    let mut at = 0;
    while at < bytes.len() {
        let mut delimiter = 0;
        while delimiter < DELIMITERS.len() && DELIMITERS[delimiter] != bytes[at] {
            delimiter += 1;
        }
        if !bytes[at].is_ascii_graphic() || delimiter < DELIMITERS.len() {
            return false;
        }
        at += 1;
    }
    !bytes.is_empty()
}

/// `value` without one pair of surrounding double quotes; usable in a
/// constant.
///
/// The quoting alone: a `\` escape inside is left as it is.
pub(crate) const fn unquoted(value: &[u8]) -> &[u8] {
    match value {
        [b'"', inner @ .., b'"'] => inner,
        _ => value,
    }
}

/// [`unquoted`] over text.
pub(crate) fn unquoted_str(text: &str) -> &str {
    let inner = unquoted(text.as_bytes()).len();
    // Either nothing or one ASCII quote was cut from each end.
    let start = (text.len() - inner) / 2;
    &text[start..start + inner]
}

/// Whether every octet of `bytes` is one a field value may hold, tab or
/// visible ASCII: what [`HeaderValue::from_static`] takes without panicking;
/// usable in a constant.
///
/// RFC 9110 section 5.5 without `obs-text`.
pub(crate) const fn is_field_value(bytes: &[u8]) -> bool {
    let mut at = 0;
    while at < bytes.len() {
        if !matches!(bytes[at], b'\t' | b' '..=b'~') {
            return false;
        }
        at += 1;
    }
    true
}

/// Copies `bytes` into `out` from `at`, returning where the copy ends; usable
/// in a constant, where a field value is assembled into a fixed buffer.
///
/// # Panics
///
/// Where `bytes` runs past `out`'s end: each caller checks the room first, by
/// its own policy.
pub(crate) const fn copy_into(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let mut index = 0;
    while index < bytes.len() {
        out[at + index] = bytes[index];
        index += 1;
    }
    at + bytes.len()
}

/// `text`'s pieces between `delimiter`s outside any `quoted-string`, left to
/// right: RFC 9110 section 5.6.6 lets a parameter value be one, and a `,` or
/// `;` inside it separates nothing. A `\` inside quotes escapes the octet
/// after it (section 5.6.4).
///
/// Shared by `Accept` and the body codecs' `Content-Type`.
pub(crate) fn unquoted_split(text: &str, delimiter: u8) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let piece = rest?;
        let (mut quoted, mut escaped) = (false, false);
        let at = piece.bytes().position(|byte| {
            if escaped {
                escaped = false;
            } else if quoted && byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = !quoted;
            } else if !quoted && byte == delimiter {
                return true;
            }
            false
        });
        match at {
            Some(at) => {
                rest = Some(&piece[at + 1..]);
                Some(&piece[..at])
            }
            None => rest.take(),
        }
    })
}

/// Whether two ASCII strings match, ignoring case.
///
/// Header names are case-insensitive per RFC 9110 section 5.1.
#[must_use]
pub(crate) const fn header_name_eq(left: &str, right: &str) -> bool {
    eq_ignore_ascii_case(left.as_bytes(), right.as_bytes())
}

/// Byte-wise ASCII case-insensitive equality, usable in a constant.
pub(crate) const fn eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut at = 0;
    while at < left.len() {
        if !left[at].eq_ignore_ascii_case(&right[at]) {
            return false;
        }
        at += 1;
    }
    true
}

/// The bytes before the first `byte`, and those after it.
pub(crate) const fn split_at_byte(bytes: &[u8], byte: u8) -> (&[u8], &[u8]) {
    let mut end = 0;
    while end < bytes.len() && bytes[end] != byte {
        end += 1;
    }
    let (before, after) = bytes.split_at(end);
    match after {
        [_, rest @ ..] => (before, rest),
        [] => (before, after),
    }
}

/// The request's own authority, from the target or from `Host`.
///
/// HTTP/2's `:authority` (RFC 9113 section 8.3.1) lands on the URI, not in the
/// map. The target wins where both are present: RFC 9112 section 3.2.2 says an
/// absolute-form target means the server "MUST ignore the received Host header
/// field".
///
/// Lowercased, and borrowed where it already was.
pub(crate) fn own_authority<'a>(
    headers: &'a HeaderMap,
    authority: Option<&'a str>,
) -> Option<std::borrow::Cow<'a, str>> {
    authority
        .or_else(|| {
            headers
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
        })
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(|host| {
            if host.bytes().any(|byte| byte.is_ascii_uppercase()) {
                std::borrow::Cow::Owned(host.to_ascii_lowercase())
            } else {
                std::borrow::Cow::Borrowed(host)
            }
        })
}

/// The field `name` in `headers`: every line, comma-joined.
///
/// RFC 9110 section 5.3. A line that is not visible ASCII is skipped, and
/// `None` means no line was left, which reads as no field at all. A single
/// line, the usual case, is borrowed.
#[must_use]
pub(crate) fn field_lines(
    headers: &HeaderMap,
    name: HeaderName,
) -> Option<std::borrow::Cow<'_, str>> {
    let mut lines = headers
        .get_all(name)
        .iter()
        .filter_map(|line| line.to_str().ok());

    let first = lines.next()?;
    let Some(second) = lines.next() else {
        return Some(std::borrow::Cow::Borrowed(first));
    };

    let mut field = format!("{first},{second}");
    for line in lines {
        field.push(',');
        field.push_str(line);
    }

    Some(std::borrow::Cow::Owned(field))
}

/// `names` as one comma-separated field value.
///
/// `None` when there is no name, or when the joined names are not a field
/// value.
pub(crate) fn join_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<HeaderValue> {
    let mut names = names.into_iter();
    let mut joined = names.next()?.to_owned();

    for name in names {
        joined.push_str(", ");
        joined.push_str(name);
    }

    HeaderValue::from_str(&joined).ok()
}

/// The description of a response with status `code`: its RFC 9110 reason
/// phrase, or ``a `{code}` response`` for a code with none.
pub(crate) fn describe_code(code: u16) -> String {
    StatusCode::from_u16(code)
        .ok()
        .and_then(|status| status.canonical_reason())
        .map_or_else(|| format!("a `{code}` response"), str::to_owned)
}

/// A delay in whole seconds, rounded *up*.
///
/// Truncating a sub-second wait to zero would tell a client to retry
/// immediately into the refusal it just received. Saturates at `u64::MAX`
/// rather than overflowing within the last second of
/// [`Duration::MAX`](std::time::Duration::MAX).
pub(crate) fn whole_seconds(delay: std::time::Duration) -> u64 {
    delay
        .as_secs()
        .saturating_add(u64::from(delay.subsec_nanos() > 0))
}

/// Describes `Retry-After`, which is a delta-seconds count or an HTTP-date.
///
/// A string, since an integer schema would be wrong for the HTTP-date form.
pub(crate) fn retry_after_header() -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
        kynos_openapi::model::schema::types::SchemaType::String,
    ))
    .with_description("How long to wait before retrying, in seconds or as an HTTP-date")
}

/// Sets `Retry-After` on `response` to `delay` in [`whole_seconds`].
pub(crate) fn set_retry_after(response: &mut Response, delay: std::time::Duration) {
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(whole_seconds(delay)));
}

/// Merges `names` into whatever `Vary` a response already carries.
///
/// A union, since `Vary` is a set several interceptors contribute to (RFC 9110
/// section 12.5.5). Names compare case-insensitively (section 5.1), every
/// `Vary` line counts (section 5.3), and `Vary: *` is never narrowed. Merged
/// over bytes so a non-UTF-8 line survives; lines are rewritten only when a
/// name is added.
///
/// Builds no line when no name is added, borrows the name when the one name
/// added is the whole field, and otherwise allocates the rewritten line once,
/// sized exactly.
pub(crate) fn vary_on(fields: &mut HeaderMap, names: &'static [&'static str]) {
    use header::VARY;

    /// Every non-empty member of every `Vary` line, trimmed.
    fn members(fields: &HeaderMap) -> impl Iterator<Item = &[u8]> {
        fields
            .get_all(VARY)
            .into_iter()
            .flat_map(|line| line.as_bytes().split(|&byte| byte == b','))
            .map(<[u8]>::trim_ascii)
            .filter(|member| !member.is_empty())
    }

    /// Each of `names` neither a member nor an earlier name, in any case.
    fn added<'a>(
        fields: &'a HeaderMap,
        names: &'static [&'static str],
    ) -> impl Iterator<Item = &'static [u8]> + 'a {
        names.iter().enumerate().filter_map(move |(index, name)| {
            let name = name.as_bytes();
            let known = members(fields)
                .chain(names[..index].iter().map(|earlier| earlier.as_bytes()))
                .any(|member| member.eq_ignore_ascii_case(name));
            (!known).then_some(name)
        })
    }

    let mut count = 0;
    let mut length = 0;

    for member in members(fields) {
        if member == b"*" {
            return;
        }
        count += 1;
        length += member.len();
    }

    let present = count;
    let mut last: &'static [u8] = b"";

    for name in added(fields, names) {
        count += 1;
        length += name.len();
        last = name;
    }

    let line = match count - present {
        0 => return,
        // The one name is the whole field, so it is borrowed rather than copied.
        1 if present == 0 => bytes::Bytes::from_static(last),
        _ => {
            let mut line = Vec::with_capacity(length + b", ".len() * (count - 1));

            for member in members(fields).chain(added(fields, names).map(|name| -> &[u8] { name }))
            {
                if !line.is_empty() {
                    line.extend_from_slice(b", ");
                }
                line.extend_from_slice(member);
            }

            // Sized exactly, so the value takes the buffer over without a copy.
            bytes::Bytes::from(line)
        }
    };

    // Unreachable for a field name; a missing cache hint beats a panicking response.
    if let Ok(value) = HeaderValue::from_maybe_shared(line) {
        fields.insert(VARY, value);
    }
}

#[cfg(test)]
mod tests;
