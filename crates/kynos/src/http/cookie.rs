//! Reading the cookies a request carries.
//!
//! Ungated: besides [`Cookies`](crate::extract::params::cookie::Cookies), a
//! cookie-carried [`SecurityScheme`](crate::security::SecurityScheme) reads a
//! jar, and must work without the `cookie` feature, which gates only the
//! parameter surface.

use crate::http::{HeaderMap, header::COOKIE};

/// Every `name=value` pair the request's `Cookie` fields carry, in order.
///
/// The pairs of every `Cookie` field, concatenated (RFC 6265 section 5.4). A
/// pair whose name or value is not ASCII is skipped, not the whole field.
///
/// Skipping means the jar cannot tell an unreadable cookie from an absent one.
/// Look a cookie up by name, a credential above all, with [`value_of`], which
/// tells them apart.
///
/// A quoted value is unwrapped. A pair with no `=` is a name with an empty
/// value.
pub fn jar(headers: &HeaderMap) -> impl Iterator<Item = (&str, &str)> + '_ {
    pairs(headers).filter_map(|(name, value)| Some((text(name)?, text(value)?)))
}

/// The first value filed under `name`.
///
/// The first, since RFC 6265 section 5.4 orders the more specific path first.
///
/// `Ok(None)` when no cookie is named `name`. A cookie whose name is not ASCII
/// is never the one asked for.
///
/// # Errors
///
/// When the first cookie named `name` has a value that is not ASCII; a later
/// cookie of the same name does not stand in for it.
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
/// What [`value_of`] reports, telling an unreadable cookie from an absent one.
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
/// Split before requiring text: every delimiter is ASCII, so a non-ASCII byte
/// stays in its own pair.
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
/// ASCII rather than UTF-8, matching [`HeaderValue::to_str`].
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
