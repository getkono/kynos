//! Reading an API key out of the request target's query string.

use super::presented::{Malformed, MalformedReason, Presented};
use crate::http::Parts;

/// The value of the one pair naming `name` in the request target's query
/// string, form-decoded as a derived
/// [`Query`](crate::extract::params::query::Query) parameter of the same name
/// is.
///
/// [`Malformed`](Presented::Malformed) with [`MalformedReason::Repeated`] when
/// two or more pairs name it, whatever their values, as a derived parameter
/// refuses a second pair: choosing either would let whatever appends a pair
/// pick the credential. Otherwise [`MalformedReason::NotText`] when the value's
/// octets are not UTF-8.
pub(super) fn value(parts: &Parts, name: &str) -> Presented<String> {
    let mut named = crate::__private::uri::query_pairs(parts.uri.query())
        .filter(|(key, _)| **key == *name.as_bytes())
        .map(|(_, value)| value);
    let Some(value) = named.next() else {
        return Presented::Absent;
    };
    if named.next().is_some() {
        return Presented::Malformed(Malformed::new(MalformedReason::Repeated));
    }
    match String::from_utf8(value.into_owned()) {
        Ok(value) => Presented::Present(value),
        Err(_) => Presented::Malformed(Malformed::new(MalformedReason::NotText)),
    }
}
