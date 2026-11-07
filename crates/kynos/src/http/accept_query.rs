//! RFC 10008 section 3's `Accept-Query`: the query media types a `QUERY`
//! operation accepts, as an RFC 9651 structured-field List.
//!
//! Built once per operation from its described `requestBody` content keys, so
//! the field names exactly the types the description promises.

use kynos_openapi::RequestBody;

use crate::http::{HeaderName, HeaderValue};

/// The field's name.
pub(crate) const ACCEPT_QUERY: HeaderName = HeaderName::from_static("accept-query");

/// The `Accept-Query` value naming every content key of `body`, in the order
/// the description lists them; `None` where no key can be a member.
///
/// A key becomes a member when it is an RFC 9110 section 12.5.1 media range
/// whose only wildcards are `*/*` and `type/*` (RFC 10008 section 3). Its
/// type and subtype are a Token where RFC 9651 section 3.3.4 admits one, and a
/// String otherwise, as section 3 asks of a type with a leading digit. Each
/// parameter becomes a structured-field Parameter of the same name, lowercased
/// since a parameter name is case-insensitive (RFC 9110 section 5.6.6), with a
/// Token or String value. A key no member can carry, such as one whose
/// parameter name is no `key` (RFC 9651 section 3.1.2), is left out whole:
/// naming its range without the parameter would advertise more than the
/// operation accepts.
pub(crate) fn from_request_body(body: &RequestBody) -> Option<HeaderValue> {
    let mut members: Vec<String> = Vec::new();
    for key in body.content.keys() {
        if let Some(member) = member(key) {
            if !members.contains(&member) {
                members.push(member);
            }
        }
    }

    crate::http::join_names(members.iter().map(String::as_str))
}

/// One content key as a List member, or `None` where it cannot be one.
fn member(key: &str) -> Option<String> {
    use std::fmt::Write as _;

    if !crate::http::media::is_media_range(key) {
        return None;
    }

    let essence = key
        .split_once(';')
        .map_or(key, |(essence, _)| essence)
        .trim_end_matches([' ', '\t']);
    let (type_, subtype) = essence.split_once('/')?;
    // `is_media_range` admits `*` inside a token, which is no wildcard section 3
    // supports.
    let wildcard = |half: &str| half != "*" && half.contains('*');
    if wildcard(type_) || wildcard(subtype) || (type_ == "*" && subtype != "*") {
        return None;
    }

    let mut member = bare_item(essence)?;
    for (name, value) in crate::http::media::parameters(key) {
        let name = name.to_ascii_lowercase();
        if !is_key(&name) {
            return None;
        }
        let value = unescaped(value);
        let _ = write!(member, ";{name}={}", bare_item(&value)?);
    }

    Some(member)
}

/// `value` as an `sf-token` where RFC 9651 section 3.3.4 admits one, else as
/// an `sf-string`; `None` where it is neither.
fn bare_item(value: &str) -> Option<String> {
    if is_sf_token(value) {
        Some(value.to_owned())
    } else {
        sf_string(value)
    }
}

/// Whether `value` is an `sf-token`: `( ALPHA / "*" ) *( tchar / ":" / "/" )`.
fn is_sf_token(value: &str) -> bool {
    let Some((&first, rest)) = value.as_bytes().split_first() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == b'*')
        && rest
            .iter()
            .all(|&byte| byte == b':' || byte == b'/' || crate::http::is_token(&[byte]))
}

/// `value` as an `sf-string` (RFC 9651 section 3.3.3), or `None` where it holds
/// an octet outside printable ASCII.
fn sf_string(value: &str) -> Option<String> {
    if !value.bytes().all(|byte| (0x20..0x7f).contains(&byte)) {
        return None;
    }

    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!("\"{escaped}\""))
}

/// Whether `name` is an RFC 9651 section 3.1.2 `key`:
/// `( lcalpha / "*" ) *( lcalpha / DIGIT / "_" / "-" / "." / "*" )`.
fn is_key(name: &str) -> bool {
    let Some((&first, rest)) = name.as_bytes().split_first() else {
        return false;
    };

    (first.is_ascii_lowercase() || first == b'*')
        && rest.iter().all(|&byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-.*".contains(&byte)
        })
}

/// A parameter value as the text it stands for: a `quoted-string` without its
/// quotes and with each `quoted-pair` resolved (RFC 9110 section 5.6.4), and a
/// token as written.
fn unescaped(value: &str) -> String {
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return value.to_owned();
    };

    let mut text = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(next) = chars.next() {
        text.push(if next == '\\' {
            chars.next().unwrap_or(next)
        } else {
            next
        });
    }
    text
}

#[cfg(test)]
mod tests;
