//! Declared request cookies.
//!
//! Splitting a jar is [`http::cookie`](crate::http::cookie)'s, not this
//! module's: a credential carried in a cookie is a
//! [`SecurityScheme`](crate::security::SecurityScheme) rather than a parameter,
//! and it reads a jar in a build where this module does not exist.

use crate::{
    error::rejection::CookieRejection,
    extract::{FromRequestParts, describe::Describe},
    http::{HeaderMap, Parts},
    router::operation::OperationCx,
    schema::registry::Registry,
};

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
    use super::CookieParams;
    use crate::{error::rejection::CookieRejection, http::HeaderMap};

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
