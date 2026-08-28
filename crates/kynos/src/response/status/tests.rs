/// A location arrives from three spellings, and the third is the reason the
/// type exists.
///
/// `relative_uri` returns an `http::Uri`, and neither that type nor `String`
/// belongs to Kynos — so without a type of its own here, the sanctioned way to
/// name another operation could not be handed to the two constructors that
/// take a location.
mod a_location_takes_every_spelling {
    use crate::response::status::{Created, Location, Redirect};

    #[test]
    fn the_three_spellings_agree() {
        let uri: crate::http::Uri = "/users/42".parse().expect("a valid reference");

        assert_eq!(Location::from("/users/42"), Location::from(uri));
        assert_eq!(
            Location::from("/users/42"),
            Location::from(String::from("/users/42"))
        );
    }

    #[test]
    fn a_typed_uri_reaches_a_created_response() {
        let uri: crate::http::Uri = "/users/42".parse().expect("a valid reference");
        let created = Created::at(uri, ());

        assert_eq!(created.location.as_str(), "/users/42");
    }

    #[test]
    fn a_typed_uri_reaches_a_redirect() {
        let uri: crate::http::Uri = "/users".parse().expect("a valid reference");
        let redirect = Redirect::<303>::to(uri);

        assert_eq!(redirect.location.as_str(), "/users");
    }

    /// A `Location` field value is a URI reference, which includes relative
    /// forms that only mean something against the request URI. Rejecting one
    /// here would refuse a value the specification permits.
    #[test]
    fn a_relative_reference_is_not_refused() {
        assert_eq!(Location::from("../sibling").as_str(), "../sibling");
    }
}

/// Every status a wrapper fixes, produced and declared.
///
/// The two halves at once, because a wrapper's whole purpose is that they
/// cannot come apart: a `Created<T>` that sent 201 and declared 200 would
/// be a response no consumer could handle, and neither half alone would
/// notice.
mod a_wrapper_produces_and_declares_one_status {
    use crate::{
        extract::body::text::Text,
        http::{StatusCode, header},
        response::{
            IntoResponse, Responses,
            status::{Accepted, Created, NoContent, Redirect},
        },
        schema::registry::Registry,
    };

    /// The statuses a type declares, as the keys a consumer would read.
    fn declared<T: Responses>() -> Vec<String> {
        let mut registry = Registry::new();
        T::responses(&mut registry)
            .responses
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn no_content_sends_204_and_no_body() {
        let response = NoContent.into_response();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(declared::<NoContent>(), ["204"]);
    }

    #[test]
    fn created_sends_201_and_says_where() {
        let response = Created::at("/users/42", Text("body".to_owned())).into_response();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/users/42")
        );

        // The wrapped body's own 200 is gone: a `Created<T>` sends 201, and
        // leaving the 200 behind would declare a response the type cannot
        // produce.
        assert_eq!(declared::<Created<Text>>(), ["201"]);
    }

    #[test]
    fn accepted_sends_202_and_keeps_the_body() {
        let response = Accepted::new(Text("body".to_owned())).into_response();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        // No `Location`: 202 says the work has not finished, so there is
        // nothing yet to point at.
        assert!(response.headers().get(header::LOCATION).is_none());
        assert_eq!(declared::<Accepted<Text>>(), ["202"]);
    }

    /// Every code the witness admits, over the whole set.
    ///
    /// A closed enumeration, so all five rather than a sample: the ones a
    /// sample would skip are 301 and 308, which differ from the other three
    /// in whether a client may rewrite the method — the one thing a caller
    /// picks a redirect code for.
    #[test]
    fn every_witnessed_redirect_code_is_the_one_it_sends() {
        fn case<const CODE: u16>()
        where
            (): crate::response::status::ValidRedirectCode<CODE>,
        {
            let response = Redirect::<CODE>::to("/elsewhere").into_response();

            assert_eq!(response.status().as_u16(), CODE);
            assert_eq!(
                response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok()),
                Some("/elsewhere")
            );
            assert_eq!(declared::<Redirect<CODE>>(), [CODE.to_string()]);
        }

        case::<301>();
        case::<302>();
        case::<303>();
        case::<307>();
        case::<308>();
    }

    /// The witness, counted against the cases above.
    #[test]
    fn every_witnessed_redirect_code_has_a_case() {
        const SOURCE: &str = include_str!("../status.rs");
        /// 301, 302, 303, 307 and 308.
        const WITNESSED: usize = 5;
        // Spelled in two pieces: `SOURCE` is this file.
        const NEEDLE: &str = concat!("impl ValidRedirect", "Code<");

        let declared = SOURCE.matches(NEEDLE).count();

        assert_eq!(
            declared, WITNESSED,
            "`status.rs` witnesses {declared} redirect code(s) and {WITNESSED} have a case"
        );
    }

    /// A redirect declares `Location` as a header a client must be able to
    /// read, because a redirect without one names nowhere to go.
    #[test]
    fn a_redirect_declares_the_location_it_sends() {
        let mut registry = Registry::new();
        let responses = <Redirect<303> as Responses>::responses(&mut registry);

        assert!(
            responses.responses["303"]
                .as_item()
                .expect("an inline response")
                .headers
                .contains_key("Location")
        );
    }
}
