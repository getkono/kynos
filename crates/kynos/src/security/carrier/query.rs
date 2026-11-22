//! Reading an API key out of the request target's query string.

use super::presented::{Malformed, MalformedReason, Presented};
use crate::http::Parts;

/// The first value of `name` in the request target's query string.
///
/// Read exactly as a derived [`Query`](crate::extract::params::query::Query)
/// parameter of the same name: form-decoded, from the first pair naming it. A
/// later pair never stands in for one that could not be read.
///
/// [`Malformed`](Presented::Malformed) when that first value's octets are not
/// UTF-8.
pub(super) fn value(parts: &Parts, name: &str) -> Presented<String> {
    let Some((_, value)) = crate::__private::uri::query_pairs(parts.uri.query())
        .find(|(key, _)| **key == *name.as_bytes())
    else {
        return Presented::Absent;
    };
    match String::from_utf8(value.into_owned()) {
        Ok(value) => Presented::Present(value),
        Err(_) => Presented::Malformed(Malformed::new(MalformedReason::NotText)),
    }
}
