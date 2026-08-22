//! Declared request cookies.
//!
//! [`jar`] and [`value_of`] are the one reader. `#[derive(CookieParams)]`
//! expands to a call rather than to a copy of the splitting rules, and anything
//! else reading a cookie -- a credential carried in one, above all -- calls the
//! same pair. RFC 6265 is small enough to inline and easy enough to inline
//! *differently*, which is the failure two copies would produce.

use crate::{
    error::rejection::CookieRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{HeaderMap, Parts, header::COOKIE},
    router::operation::OperationCx,
    schema::registry::Registry,
};

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

/// Declared request cookies.
///
/// `T` derives `Cookies`. There is no whole-jar extractor; a cookie carrying
/// credentials is a [`SecurityScheme`](crate::security::SecurityScheme), not a
/// parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cookies<T>(pub T);

/// A group of request cookies.
pub trait CookieParams: Sized {
    /// The cookie names this group declares.
    const NAMES: &'static [&'static str];

    /// Decodes this group from the request's cookie header fields.
    ///
    /// The whole [`HeaderMap`] rather than the `Cookie` field alone, because a
    /// request may carry more than one and the jar is their concatenation.
    ///
    /// # Panics
    ///
    /// The default panics. Derive `CookieParams`, or write this by hand, before
    /// extracting the group.
    fn decode(headers: &HeaderMap) -> Result<Self, CookieRejection> {
        let _ = headers;
        unimplemented!(
            "`{}` does not decode cookies: derive `CookieParams` on it, or implement `decode` by \
             hand",
            std::any::type_name::<Self>()
        )
    }

    /// Describes the declared OpenAPI cookie parameters.
    ///
    /// The default describes the declared [`NAMES`](CookieParams::NAMES) with an
    /// unconstrained schema, and marks none of them required: a group that has
    /// not said which cookies a request must carry has not said they all are.
    ///
    /// `style` is left unstated: `form` is the default for a cookie parameter,
    /// so stating it would only repeat the location.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        Self::NAMES
            .iter()
            .copied()
            .map(|name| kynos_openapi::Parameter::cookie(name, kynos_openapi::Schema::any()))
            .collect()
    }
}

impl<C: Sync, T: CookieParams + Send> FromRequestParts<C> for Cookies<T> {
    type Rejection = CookieRejection;

    async fn from_request_parts(parts: &mut Parts, _context: &C) -> Result<Self, Self::Rejection> {
        T::decode(&parts.headers).map(Cookies)
    }
}

impl<T: CookieParams> Describe for Cookies<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let parameters = T::parameters(operation.registry());
        for parameter in parameters {
            operation.add_parameter(parameter);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CookieParams, jar, value_of};
    use crate::{error::rejection::CookieRejection, http::HeaderMap};

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
        let cases: &[(&[&str], &[(&str, &str)])] = &[
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

    /// A group that has said nothing about how it is spelled.
    #[derive(Debug)]
    struct Unspelled;

    impl CookieParams for Unspelled {
        const NAMES: &'static [&'static str] = &["session"];
    }

    #[test]
    #[should_panic(expected = "does not decode cookies")]
    fn a_group_that_declares_no_decoder_says_so() {
        let _ = Unspelled::decode(&HeaderMap::new());
    }

    /// The control, which also pins the reason the signature takes the whole
    /// map: a request may carry more than one `Cookie` field, and the jar is
    /// their concatenation rather than the first of them.
    #[test]
    fn a_group_that_declares_a_decoder_sees_every_cookie_field() {
        #[derive(Debug, PartialEq)]
        struct Recorded(Vec<String>);

        impl CookieParams for Recorded {
            const NAMES: &'static [&'static str] = &["session"];

            fn decode(headers: &HeaderMap) -> Result<Self, CookieRejection> {
                Ok(Self(
                    headers
                        .get_all(crate::http::header::COOKIE)
                        .iter()
                        .map(|value| value.to_str().expect("a printable field").to_owned())
                        .collect(),
                ))
            }
        }

        let mut headers = HeaderMap::new();
        headers.append(
            crate::http::header::COOKIE,
            crate::http::HeaderValue::from_static("a=1"),
        );
        headers.append(
            crate::http::header::COOKIE,
            crate::http::HeaderValue::from_static("b=2"),
        );

        assert_eq!(
            Recorded::decode(&headers).expect("decoded"),
            Recorded(vec!["a=1".to_owned(), "b=2".to_owned()])
        );
    }

    /// The description names every declared cookie and marks none required: a
    /// group that has not said which cookies a request must carry has not said
    /// they all are.
    #[test]
    fn the_default_description_requires_no_cookie_it_names() {
        let mut registry = crate::schema::registry::Registry::new();
        let parameters = Unspelled::parameters(&mut registry);

        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].name, "session");
        assert_eq!(parameters[0].required, None);
    }
}
