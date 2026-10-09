//! Reading an API key out of the request target's query string.

use crate::{error::rejection::AuthRejection, http::Parts};

/// The first value of `name` in the request target's query string.
///
/// Read exactly as a derived [`Query`](crate::extract::params::query::Query)
/// parameter of the same name: form-decoded, from the first pair naming it. A
/// later pair never stands in for one that could not be read.
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
