//! Reading the cookies a request carries.
//!
//! Here rather than beside [`Cookies`](crate::extract::params::cookie::Cookies)
//! because two unrelated things read a jar and only one of them is a parameter.
//! A credential carried in a cookie is a
//! [`SecurityScheme`](crate::security::SecurityScheme), and it has to work in a
//! build with no `cookie` feature.
//!
//! That used to be argued as "the feature names a *dependency*, and RFC 6265's
//! splitting rules need none". The premise is gone: the `cookie` crate was
//! removed and the feature names no dependency at all. The conclusion stands on
//! its own — what `cookie` gates is the *parameter* surface, and a cookie
//! credential is a security scheme rather than a parameter, so gating this
//! would put a `SecurityScheme` out of reach of a build that can still declare
//! one.

use crate::http::{HeaderMap, header::COOKIE};

/// Every `name=value` pair the request's `Cookie` fields carry, in order.
///
/// A request may carry more than one `Cookie` field and each may hold more than
/// one pair, so the jar is the concatenation of both -- RFC 6265 section 5.4.
/// A pair whose name or value is not ASCII is skipped rather than failing the
/// whole jar, since one unreadable cookie must not hide the rest. The pair is
/// what is skipped, not the field it travels in: section 5.4 has a client send
/// one `Cookie` field, so a field is usually the whole jar.
///
/// Skipping a pair means the jar cannot tell a cookie it could not read from
/// one that was never sent, and a later cookie of the same name can come first
/// in it. Look a cookie up by name, a credential above all, with [`value_of`],
/// which tells them apart.
///
/// A value written in RFC 6265's quoted form is unwrapped: the quotes delimit
/// the value rather than belonging to it. A pair with no `=` is a name with an
/// empty value, which is what a client sending a bare flag produces.
pub fn jar(headers: &HeaderMap) -> impl Iterator<Item = (&str, &str)> + '_ {
    pairs(headers).filter_map(|(name, value)| Some((text(name)?, text(value)?)))
}

/// The first value filed under `name`.
///
/// The first rather than the last: RFC 6265 section 5.4 orders a jar by
/// specificity, so where a client sends two cookies of one name the earlier is
/// the one for the more specific path.
///
/// `Ok(None)` when no cookie is named `name`. A cookie whose name is not ASCII
/// is never the one asked for.
///
/// # Errors
///
/// When the first cookie named `name` has a value that is not ASCII. It was
/// sent, so it is not absent, and a later cookie of the same name does not
/// stand in for it.
pub fn value_of<'r>(headers: &'r HeaderMap, name: &str) -> Result<Option<&'r str>, Unreadable> {
    let Some((_, value)) =
        pairs(headers).find(|(found, _)| text(found).is_some_and(|found| found == name))
    else {
        return Ok(None);
    };
    text(value).map(Some).ok_or(Unreadable)
}

/// A cookie was sent under the name asked for, and its value is not ASCII.
///
/// What [`value_of`] reports in place of the value, so that a cookie that could
/// not be read is told apart from one that was never sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable;

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the cookie value is not ASCII")
    }
}

impl std::error::Error for Unreadable {}

/// Every pair of every `Cookie` field, as octets.
///
/// Split before anything is required to be text: every delimiter the grammar
/// uses is ASCII, so a byte above it cannot be mistaken for one and stays in
/// the pair that carries it.
fn pairs(headers: &HeaderMap) -> impl Iterator<Item = (&[u8], &[u8])> + '_ {
    headers
        .get_all(COOKIE)
        .into_iter()
        .flat_map(|field| field.as_bytes().split(|&byte| byte == b';'))
        .filter_map(|entry| {
            let entry = entry.trim_ascii();
            if entry.is_empty() {
                return None;
            }
            let (name, value) = match entry.iter().position(|&byte| byte == b'=') {
                Some(at) => (&entry[..at], &entry[at + 1..]),
                None => (entry, &[][..]),
            };
            let value = value.trim_ascii();
            let value = value
                .strip_prefix(b"\"")
                .and_then(|value| value.strip_suffix(b"\""))
                .unwrap_or(value);
            Some((name.trim_ascii(), value))
        })
}

/// `octets` as text, when every one of them is ASCII.
///
/// ASCII rather than UTF-8 because it is what [`HeaderValue::to_str`] accepts:
/// a field holds no control but tab, so the one thing that refuses is a byte
/// above 0x7F, and a cookie is read as the header carrying it would be.
///
/// [`HeaderValue::to_str`]: crate::http::HeaderValue::to_str
fn text(octets: &[u8]) -> Option<&str> {
    if octets.is_ascii() {
        std::str::from_utf8(octets).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
