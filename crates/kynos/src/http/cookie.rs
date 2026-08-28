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
/// A field that is not printable ASCII is skipped rather than failing the whole
/// jar, since one unreadable cookie must not hide the rest.
///
/// A value written in RFC 6265's quoted form is unwrapped: the quotes delimit
/// the value rather than belonging to it. A pair with no `=` is a name with an
/// empty value, which is what a client sending a bare flag produces.
pub fn jar(headers: &HeaderMap) -> impl Iterator<Item = (&str, &str)> + '_ {
    headers
        .get_all(COOKIE)
        .into_iter()
        .filter_map(|field| field.to_str().ok())
        .flat_map(|text| text.split(';'))
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            Some((name.trim(), value))
        })
}

/// The first value filed under `name`.
///
/// The first rather than the last: RFC 6265 section 5.4 orders a jar by
/// specificity, so where a client sends two cookies of one name the earlier is
/// the one for the more specific path.
#[must_use]
pub fn value_of<'r>(headers: &'r HeaderMap, name: &str) -> Option<&'r str> {
    jar(headers).find_map(|(found, value)| (found == name).then_some(value))
}

#[cfg(test)]
mod tests;
