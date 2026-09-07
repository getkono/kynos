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
/// What the wrapper may carry across is bounded by the count: exactly one
/// response, because one response is the whole of what the body sends and
/// anything more is several exchanges arriving under a single key.
///
/// The bodies here are hand-written rather than derived, because `Responses`
/// is the whole of what the wrapper reads and a derive would put the macro
/// crate between the assertion and the rule. The arrangement they stand in for
/// exists in the tree already: `tests/derives.rs`'s `CreateReply` declares a
/// 201 carrying a `User` and a 409 carrying nothing, so `Created<CreateReply>`
/// is a `Created` over a body that never described a 200 -- and, with two
/// responses of which one is bodiless, one the wrapper declares nothing for.
mod a_wrapper_declares_the_body_it_forwards {
    use crate::{
        http::{Response, StatusCode, body::Body, header},
        response::{
            IntoResponse, Responses,
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

    /// One response, filed under a status of the body's own choosing.
    ///
    /// The shape `#[derive(Reply)]` produces for a single variant naming its
    /// own status. One response is the whole of what the body sends, so the
    /// wrapper re-describing it promises nothing a value of the body withholds.
    struct OneRepresentation;

    impl Responses for OneRepresentation {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new().with(201, json("the resource as stored", registry))
        }
    }

    /// A representation beside a status carrying none.
    ///
    /// The shape `#[derive(Reply)]` produces for `{204: none, 409: content}`.
    /// Both variants reach the wire under the wrapper's status, so the two
    /// entries are two different exchanges under one key -- and only one of
    /// them carries a representation.
    struct ARepresentationAndABodilessStatus;

    impl Responses for ARepresentationAndABodilessStatus {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new()
                .with(204, kynos_openapi::Response::new("it was replaced"))
                .with(409, json("what is already there", registry))
        }
    }

    /// The bodiless half of the type above, which is the half a wrapper's
    /// declaration has to survive: `Created<T>` overwrites the 204 with 201 and
    /// forwards this body unchanged, so the exchange is a 201 carrying neither
    /// octets nor a `Content-Type`.
    impl IntoResponse for ARepresentationAndABodilessStatus {
        fn into_response(self) -> Response {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NO_CONTENT;
            response
        }
    }

    /// Two representations, so which one a wrapper would take is a guess.
    ///
    /// The shape a `Reply` enum with two content-bearing variants produces. No
    /// composition Kynos ships reaches it: `Ranged<T>`, `RangedParts<T>` and
    /// `Delivery<M>` all declare a 200, so a wrapper over one of them re-keys
    /// that 200 instead of ever asking what else the body declared.
    struct TwoRepresentations;

    impl Responses for TwoRepresentations {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new()
                .with(206, json("a range of it", registry))
                .with(416, json("the range is unsatisfiable", registry))
        }
    }

    /// One response, carrying nothing.
    ///
    /// The shape a `Reply` enum with a single bodiless variant produces, and
    /// the body a wrapper must not invent a representation for.
    struct NoRepresentationAtAll;

    impl Responses for NoRepresentationAtAll {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            let _ = registry;
            kynos_openapi::Responses::new().with(204, kynos_openapi::Response::new("it was done"))
        }
    }

    /// No response at all, which is what a body declaring nothing hands over.
    struct NothingDeclared;

    impl Responses for NothingDeclared {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            let _ = registry;
            kynos_openapi::Responses::new()
        }
    }

    /// The one response, filed under `default` rather than under a status.
    struct SoleRepresentationUnderDefault;

    impl Responses for SoleRepresentationUnderDefault {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new().with_default(json("whatever it answers with", registry))
        }
    }

    /// One response, and it is a `$ref` whose content this module cannot read.
    struct OnlyAReference;

    impl Responses for OnlyAReference {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            let _ = registry;
            kynos_openapi::Responses::new().with_pattern(
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

    /// A body with nothing to carry across leaves the wrapper declaring
    /// nothing, which is what the wrapper then sends.
    ///
    /// Two ways to have nothing: one response carrying no content, and no
    /// response at all. Both are the honest empty declaration rather than the
    /// disagreement above, and neither is reached by any composition in the
    /// tree -- so this is what says the fallback stays out of their way.
    #[test]
    fn a_body_with_no_representation_leaves_the_wrapper_empty() {
        assert!(representations::<Created<NoRepresentationAtAll>>(201).is_empty());
        assert!(representations::<Created<NothingDeclared>>(201).is_empty());
    }

    /// A `default` is the body's response for every status it did not name, so
    /// it is as much a thing the body puts on the wire as a keyed one -- and
    /// when it is the only one, it is what the wrapper's status sends.
    #[test]
    fn a_sole_default_is_the_representation_the_wrapper_takes() {
        assert_eq!(
            representations::<Created<SoleRepresentationUnderDefault>>(201),
            ["application/json"]
        );
    }

    /// A `$ref` names a response the document holds elsewhere, so whether it
    /// carries content is not a question this module can answer -- and a
    /// representation the wrapper cannot read is one it cannot promise.
    #[test]
    fn a_sole_reference_leaves_the_wrapper_empty() {
        assert!(representations::<Created<OnlyAReference>>(201).is_empty());
    }

    /// A representation is the wrapper's to declare only where every value it
    /// wraps sends one.
    ///
    /// `{204: none, 409: content}` holds one content-bearing response and no
    /// 200, so counting content-bearing responses alone finds "nothing to
    /// choose between" and carries `application/json` onto the 201. The value
    /// below is the other variant: it reaches the wire as a 201 with no
    /// `Content-Type` and no octets, under a description promising a JSON
    /// document. `assert_conformance` reports that pair as "no `Content-Type`
    /// was sent, but the description declares application/json" -- the
    /// disagreement in the direction the wrapper is meant to close, not open.
    #[test]
    fn a_bodiless_status_beside_a_representation_leaves_the_wrapper_empty() {
        let sent = Created::at("/things/1", ARepresentationAndABodilessStatus).into_response();

        assert_eq!(sent.status(), StatusCode::CREATED);
        assert!(
            sent.headers().get(header::CONTENT_TYPE).is_none(),
            "the bodiless variant reaches 201 carrying no representation"
        );
        assert!(
            representations::<Created<ARepresentationAndABodilessStatus>>(201).is_empty(),
            "the wrapper declares a representation the bodiless variant never sends"
        );
    }
}

/// A body that already declares the status its wrapper fixes.
///
/// The wrapper knows the status and what the status *means*; the body knows
/// what it sends. Where the two meet on one key the body's half is the one
/// that cannot be reconstructed, so it is the one that survives -- and the
/// wrapper adds only what it contributes itself.
///
/// The shape is `#[derive(Reply)]`'s: a variant naming 201 beside a variant
/// naming 409, which is what an operation that either creates or conflicts
/// writes. `crates/kynos/tests/derives.rs` already declares it.
mod a_wrapper_keeps_what_the_body_declared_for_its_own_status {
    use crate::{
        response::{
            Responses,
            status::{Accepted, Created},
        },
        schema::registry::Registry,
    };

    fn declared<T: Responses>(status: u16) -> kynos_openapi::Response {
        let mut registry = Registry::new();
        T::responses(&mut registry)
            .get(status)
            .and_then(kynos_openapi::RefOr::as_item)
            .cloned()
            .unwrap_or_else(|| panic!("a response is declared for {status}"))
    }

    fn json(description: &str, registry: &mut Registry) -> kynos_openapi::Response {
        kynos_openapi::Response::with_content(
            description,
            "application/json",
            kynos_openapi::MediaType::new(registry.resolve::<String>()),
        )
    }

    /// `{201: content, 409: content}` -- the wrapper's own status, beside one
    /// the wrapper never touches.
    struct CreateReply;

    impl Responses for CreateReply {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new()
                .with(201, json("the user as created", registry))
                .with(409, json("that email is already registered", registry))
        }
    }

    /// The same shape one status along, for `Accepted<T>`.
    struct AcceptReply;

    impl Responses for AcceptReply {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new()
                .with(202, json("the job as queued", registry))
                .with(409, json("a job is already running", registry))
        }
    }

    /// One response, under a wildcard rather than a status.
    ///
    /// `4XX` is a range, so it is a claim about no single status -- and a
    /// wrapper that took it would declare an error representation as what its
    /// 201 sends.
    struct SoleWildcard;

    impl Responses for SoleWildcard {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new().with_pattern(
                kynos_openapi::StatusPattern::ClientError,
                kynos_openapi::RefOr::Item(json("something the client did", registry)),
            )
        }
    }

    /// One response carrying a required header beside its representation.
    struct SoleResponseWithAHeader;

    impl Responses for SoleResponseWithAHeader {
        fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
            kynos_openapi::Responses::new().with(
                409,
                json("what is already there", registry).with_header(
                    "X-Trace",
                    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
                        kynos_openapi::model::schema::types::SchemaType::String,
                    ))
                    .required(true),
                ),
            )
        }
    }

    #[test]
    fn the_representation_the_body_declared_survives() {
        let created = declared::<Created<CreateReply>>(201);

        assert_eq!(
            created.content.keys().collect::<Vec<_>>(),
            ["application/json"],
            "the body's 201 carried a representation and the wrapper replaced it"
        );
    }

    /// The description too, and this is the half a re-key would silently drop:
    /// "the user as created" is the sentence an author wrote for this exact
    /// status, and "the resource was created" is the generic one.
    #[test]
    fn the_description_the_body_declared_survives() {
        assert_eq!(
            declared::<Created<CreateReply>>(201).description.as_deref(),
            Some("the user as created")
        );
    }

    /// What the wrapper does contribute still arrives: `Location` is the
    /// wrapper's own statement and no body declares it.
    #[test]
    fn the_wrappers_own_header_is_added_to_the_kept_entry() {
        assert!(
            declared::<Created<CreateReply>>(201)
                .headers
                .contains_key("Location")
        );
    }

    /// A status the wrapper does not fix was never its to touch.
    #[test]
    fn the_bodys_other_status_is_left_alone() {
        let conflict = declared::<Created<CreateReply>>(409);

        assert_eq!(
            conflict.content.keys().collect::<Vec<_>>(),
            ["application/json"]
        );
        assert_eq!(
            conflict.description.as_deref(),
            Some("that email is already registered")
        );
    }

    #[test]
    fn accepted_keeps_what_the_body_declared_for_202() {
        let accepted = declared::<Accepted<AcceptReply>>(202);

        assert_eq!(
            accepted.content.keys().collect::<Vec<_>>(),
            ["application/json"]
        );
        assert_eq!(accepted.description.as_deref(), Some("the job as queued"));
    }

    /// A wildcard is a range rather than a claim about one status, so there is
    /// nothing in it the wrapper's single status may borrow.
    #[test]
    fn a_sole_wildcard_is_not_a_representation_the_wrapper_takes() {
        assert!(declared::<Created<SoleWildcard>>(201).content.is_empty());
    }

    /// The re-key arm carries content, headers and links by reusing the whole
    /// response; the carry-over arm reached for the content alone, so a header
    /// the body declares required went undeclared while the body still sent it.
    #[test]
    fn a_carried_over_response_brings_its_headers() {
        assert!(
            declared::<Created<SoleResponseWithAHeader>>(201)
                .headers
                .contains_key("X-Trace")
        );
    }
}
