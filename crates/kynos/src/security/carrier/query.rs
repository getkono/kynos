//! Reading an API key out of the request target's query string.

use crate::{error::rejection::AuthRejection, http::Parts};

/// The first value of `name` in the request target's query string.
///
/// An API key `in: query` is a query parameter, so it is read exactly as a
/// derived [`Query`](crate::extract::params::query::Query) parameter of the same
/// name is: through the same decoder, which applies the form rules OpenAPI
/// requires of every `in: query` parameter (`+` is a space, `%2B` a plus sign),
/// and from the first pair that names it. A later pair never stands in for one
/// that could not be read, since two readers of one request would then pick
/// different credentials.
///
/// # Errors
///
/// When that first value's octets are not UTF-8: present and malformed.
pub(super) fn value(parts: &Parts, name: &str) -> Result<Option<String>, AuthRejection> {
    let Some((_, value)) = crate::__private::uri::query_pairs(parts.uri.query())
        .find(|(key, _)| **key == *name.as_bytes())
    else {
        return Ok(None);
    };
    String::from_utf8(value.into_owned())
        .map(Some)
        .map_err(|_| AuthRejection::unauthenticated())
}
