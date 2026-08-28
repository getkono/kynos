use super::CookieParams;
use crate::{error::rejection::CookieRejection, http::HeaderMap};

// A `#[should_panic]` case stood here, asserting that a group declaring no
// decoder said so at run time. `decode` is required now — a cookie group is
// only ever read, so there was never a one-direction case to keep a default
// for — and a group without one does not compile. The control below stays.

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
    /// A group that says what it is named and nothing more.
    struct Named;

    impl CookieParams for Named {
        const NAMES: &'static [&'static str] = &["session"];

        fn decode(_: &HeaderMap) -> Result<Self, CookieRejection> {
            Ok(Self)
        }
    }

    let mut registry = crate::schema::registry::Registry::new();
    let parameters = Named::parameters(&mut registry);

    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].name, "session");
    assert_eq!(parameters[0].required, None);
}
