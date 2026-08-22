//! Reading the cookies a request carries.
//!
//! Here rather than beside [`Cookies`](crate::extract::params::cookie::Cookies)
//! because two unrelated things read a jar and only one of them is a parameter.
//! A credential carried in a cookie is a
//! [`SecurityScheme`](crate::security::SecurityScheme), and it has to work in a
//! build with no `cookie` feature — the feature names a *dependency*, and RFC
//! 6265's splitting rules need none.

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
mod tests {
    use super::{jar, value_of};
    use crate::http::HeaderMap;

    /// A jar built from the `Cookie` fields `fields` holds.
    fn from(fields: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for field in fields {
            headers.append(
                crate::http::header::COOKIE,
                crate::http::HeaderValue::from_str(field).expect("a printable field"),
            );
        }
        headers
    }

    /// Every shape RFC 6265 section 4.2.1 permits in a `Cookie` field, swept
    /// rather than sampled.
    ///
    /// The space is small and closed -- a pair is a name, an optional `=`, and
    /// a value that may be quoted -- so enumerating it is the stronger
    /// statement. Each row is what a client actually sends; the expectation
    /// beside it is written from the grammar rather than from what the reader
    /// happens to do.
    #[test]
    fn a_jar_is_split_the_way_the_grammar_writes_it() {
        /// The `Cookie` fields a client sent, and the pairs they hold.
        type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)]);

        let cases: &[Case<'_>] = &[
            (&["a=1"], &[("a", "1")]),
            // Several pairs in one field, and the separator is `; `.
            (&["a=1; b=2"], &[("a", "1"), ("b", "2")]),
            // Several fields, concatenated in order.
            (&["a=1", "b=2"], &[("a", "1"), ("b", "2")]),
            // Whitespace around either half is not part of it.
            (&["  a = 1  "], &[("a", "1")]),
            // A quoted value: the quotes delimit rather than belong.
            (&[r#"a="1""#], &[("a", "1")]),
            // One quote is not a pair of them, so nothing is stripped.
            (&[r#"a="1"#], &[("a", "\"1")]),
            // A bare name is a name with an empty value.
            (&["flag"], &[("flag", "")]),
            // An explicitly empty value is the same thing spelled out.
            (&["a="], &[("a", "")]),
            // Empty entries are skipped rather than yielding empty names.
            (&["a=1;;b=2"], &[("a", "1"), ("b", "2")]),
            (&[";"], &[]),
            // A value may hold `=`; only the first splits.
            (&["token=ab=cd"], &[("token", "ab=cd")]),
            // No field at all is an empty jar, not an error.
            (&[], &[]),
        ];

        for (fields, expected) in cases {
            let headers = from(fields);
            let read: Vec<_> = jar(&headers).collect();
            assert_eq!(&read.as_slice(), expected, "reading {fields:?}");
        }
    }

    /// A field no `&str` can hold is skipped, and the rest of the jar survives.
    ///
    /// One unreadable cookie hiding every other one would turn a client's
    /// mistake into the service losing a session it was sent.
    #[test]
    fn an_unprintable_field_does_not_hide_the_others() {
        let mut headers = from(&["a=1"]);
        headers.append(
            crate::http::header::COOKIE,
            crate::http::HeaderValue::from_bytes(b"b=\xff").expect("a legal field value"),
        );
        headers.append(
            crate::http::header::COOKIE,
            crate::http::HeaderValue::from_static("c=3"),
        );

        let read: Vec<_> = jar(&headers).collect();
        assert_eq!(read, [("a", "1"), ("c", "3")]);
    }

    /// RFC 6265 section 5.4 orders a jar most-specific first, so where a client
    /// sends one name twice the earlier is the one for the narrower path.
    #[test]
    fn a_repeated_name_reads_back_as_the_first_one_sent() {
        let headers = from(&["session=narrow", "session=wide"]);
        assert_eq!(value_of(&headers, "session"), Some("narrow"));
    }

    #[test]
    fn a_name_the_jar_does_not_hold_reads_back_as_absent() {
        let headers = from(&["a=1"]);
        assert_eq!(value_of(&headers, "b"), None);
    }
}
