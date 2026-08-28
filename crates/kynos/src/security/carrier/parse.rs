//! Reading the `Authorization` field, per RFC 9110 section 11.6.2.

use crate::{
    error::rejection::AuthRejection,
    http::{Parts, header::AUTHORIZATION},
};

/// The two halves of an `Authorization` field value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Authorization<'r> {
    /// The RFC 9110 section 11.1 scheme token, as the client spelled it.
    pub(super) scheme: &'r str,
    /// Everything after the single space that follows the scheme.
    pub(super) credentials: &'r str,
}

/// Reads the request's `Authorization` field.
///
/// `Ok(None)` when there is none, which is anonymity rather than a failure —
/// the caller decides whether that is acceptable. `Err` when there is one and
/// it is not a credential: two fields, bytes no `&str` can hold, or a value
/// with no scheme token.
pub(super) fn authorization(parts: &Parts) -> Result<Option<Authorization<'_>>, AuthRejection> {
    let mut fields = parts.headers.get_all(AUTHORIZATION).into_iter();

    let Some(field) = fields.next() else {
        return Ok(None);
    };

    // RFC 9110 section 5.3 makes `Authorization` a singleton field. Two of them
    // is not a credential to choose between: picking either would mean a proxy
    // that appended one could decide which credential a service honours.
    if fields.next().is_some() {
        return Err(AuthRejection::unauthenticated());
    }

    let value = field
        .to_str()
        .map_err(|_| AuthRejection::unauthenticated())?;

    // `scheme SP credentials`, and the scheme is a token so it holds no space.
    let (scheme, credentials) = value
        .split_once(' ')
        .ok_or_else(AuthRejection::unauthenticated)?;

    if scheme.is_empty() {
        return Err(AuthRejection::unauthenticated());
    }

    Ok(Some(Authorization {
        scheme,
        // RFC 9110 section 11.6.2 permits bad whitespace after the scheme.
        credentials: credentials.trim_start_matches(' '),
    }))
}

/// Whether `presented` names `expected`.
///
/// RFC 9110 section 11.1 makes an authentication scheme name case-insensitive,
/// so `bearer`, `Bearer` and `BEARER` are one scheme. Getting this wrong is not
/// pedantry: a client that spells it in lower case is one whose credential a
/// case-sensitive comparison silently ignores.
pub(super) fn scheme_is(presented: &str, expected: &str) -> bool {
    presented.eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests;
