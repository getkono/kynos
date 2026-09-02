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

    /// The representations a type declares under one status.
    ///
    /// The keys alone say which statuses exist and nothing about what each
    /// carries, so a wrapper that lost the body's representation while
    /// re-keying it would satisfy every assertion above.
    fn representations<T: Responses>(status: u16) -> Vec<String> {
        let mut registry = Registry::new();
        T::responses(&mut registry)
            .get(status)
            .and_then(kynos_openapi::RefOr::as_item)
            .map(|response| response.content.keys().cloned().collect())
            .unwrap_or_default()
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
        // Re-keyed with its representation, not merely re-keyed.
        assert_eq!(representations::<Created<Text>>(201), ["text/plain"]);
    }

    #[test]
    fn accepted_sends_202_and_keeps_the_body() {
        let response = Accepted::new(Text("body".to_owned())).into_response();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        // No `Location`: 202 says the work has not finished, so there is
        // nothing yet to point at.
        assert!(response.headers().get(header::LOCATION).is_none());
        assert_eq!(declared::<Accepted<Text>>(), ["202"]);
        assert_eq!(representations::<Accepted<Text>>(202), ["text/plain"]);
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

/// What a wrapper declares for a body that never described a 200.
///
/// `Created<T>` and `Accepted<T>` overwrite the status on whatever
/// `T::into_response` produced, so `T`'s representation reaches the wire under
/// the wrapper's status whichever key `T` filed it under. Declaring nothing
/// there is a description of an empty response over a body -- exactly what
/// `assert_conformance` reports as "the description declares no content, but a
/// 35-byte body was sent".
///
/// The bodies here are hand-written rather than derived, because `Responses`
/// is the whole of what the wrapper reads and a derive would put the macro
/// crate between the assertion and the rule. The arrangement they stand in for
/// exists in the tree already: `tests/derives.rs`'s `CreateReply` declares a
/// 201 carrying a `User` and a 409 carrying nothing, so `Created<CreateReply>`
/// is a `Created` over a body that never described a 200.
mod a_wrapper_declares_the_body_it_forwards {
    use crate::{
        response::{
            Responses,
            status::{Accepted, Created},
        },
        schema::registry::Registry,
    };

    /// The representations a type declares under one status.
    fn representations<T: Responses>(status: u16) -> Vec<String> {
        let mut registry = Registry::new();
        T::responses(&mut registry)
            .get(status)
            .and_then(kynos_openapi::RefOr::as_item)
            .map(|response| response.content.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn json(description: &str, registry: &mut Registry) -> kynos_openapi::Response {
        kynos_openapi::Response::with_content(
            description,
            "application/json",
            kynos_openapi::MediaType::new(registry.resolve::<String>()),
        )
    }

    /// One representation, filed under a status of the body's own choosing.
    ///
    /// The shape `#[derive(Reply)]` produces for `{201: content, 409: none}`,
    /// which is the arrangement issue #104's neighbourhood was found in.
    struct OneRepresentation;

    impl Responses for OneRepresentation {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new()
                .with(201, json("the resource as stored", registry))
                .with(409, kynos_openapi::Response::new("it already exists"))
        }
    }

    /// Two representations, so which one a wrapper would take is a guess.
    ///
    /// The shape `Created<Ranged<Json<T>>>` and `Created<Delivery<M>>` reach:
    /// several content-bearing statuses, none of them 200.
    struct TwoRepresentations;

    impl Responses for TwoRepresentations {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new()
                .with(206, json("a range of it", registry))
                .with(416, json("the range is unsatisfiable", registry))
        }
    }

    /// One inline representation beside a `$ref`, whose content this module
    /// cannot read.
    struct OneRepresentationAndAReference;

    impl Responses for OneRepresentationAndAReference {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new()
                .with(201, json("the resource as stored", registry))
                .with_pattern(
                    kynos_openapi::StatusPattern::Code(409),
                    kynos_openapi::Ref::new("#/components/responses/Conflict").into(),
                )
        }
    }

    #[test]
    fn created_takes_the_one_representation_the_body_declares() {
        assert_eq!(
            representations::<Created<OneRepresentation>>(201),
            ["application/json"]
        );
    }

    #[test]
    fn accepted_takes_the_one_representation_the_body_declares() {
        assert_eq!(
            representations::<Accepted<OneRepresentation>>(202),
            ["application/json"]
        );
    }

    /// The other half of the rule, which is what keeps it from guessing.
    ///
    /// Two content-bearing statuses and no 200: any choice between them would
    /// be the wrapper inventing a representation the body never promised for
    /// that status, so it declares none and the description stays honest about
    /// knowing nothing.
    #[test]
    fn a_body_declaring_two_representations_leaves_the_wrapper_empty() {
        assert!(representations::<Created<TwoRepresentations>>(201).is_empty());
    }

    /// A `$ref` names a response the document holds elsewhere, so whether it
    /// carries content is not a question this module can answer -- and "exactly
    /// one" cannot be established over a set with an unreadable member.
    #[test]
    fn a_reference_beside_a_representation_leaves_the_wrapper_empty() {
        assert!(representations::<Created<OneRepresentationAndAReference>>(201).is_empty());
    }
}
