use crate::{
    emit::downgrade,
    model::{
        document::{Document, SpecVersion},
        info::Info,
    },
};

fn document() -> Document {
    Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
}

#[test]
fn a_bare_document_emits_only_the_required_fields() {
    let json = document().to_json().expect("serializable");
    assert!(json.contains(r#""openapi": "3.1.2""#));
    assert!(json.contains(r#""title": "Orders""#));
    assert!(!json.contains("paths"));
    assert!(!json.contains("components"));
}

#[test]
fn emitting_the_declared_version_is_a_no_op() {
    let emitted = document()
        .emit(SpecVersion::V3_1)
        .expect("no 3.2 constructs");
    assert_eq!(emitted.openapi, "3.1.2");
}

#[test]
fn a_document_using_no_three_two_construct_has_no_blockers() {
    assert!(downgrade::three_two_only_constructs(&document()).is_empty());
}

/// One case per 3.2-only construct, and the exact location it is reported at.
///
/// `properties.rs` checks emission against `three_two_only_constructs` itself:
/// `emit(V3_1)` succeeds exactly when that function returns nothing, and the
/// error carries whatever it returned. So the property agrees with the function
/// by construction, including where the function is wrong. A construct it
/// forgets to look for is a document that silently emits as 3.1 while using a
/// keyword 3.1 has no way to read — and every test in the crate passes.
///
/// These cases are what the property cannot be: each builds a document using
/// exactly one 3.2 construct and states, independently, where that construct
/// lives. The location strings are the product here, since they are what a
/// caller is shown.
#[cfg(feature = "openapi32")]
mod blockers {
    use super::document;
    use crate::{
        emit::downgrade::three_two_only_constructs,
        model::{
            body::{encoding::Encoding, media_type::MediaType},
            components::ComponentName,
            document::{Document, SpecVersion},
            parameter::{Parameter, ParameterIn, style::Style},
            paths::{item::PathItem, method::Method, operation::Operation, template::PathTemplate},
            reference::RefOr,
            response::{Response, Responses},
            schema::Schema,
            security::{
                SecurityScheme,
                oauth::{OAuthFlow, OAuthFlows},
            },
            server::Server,
            tag::Tag,
        },
        validate::violation::SpecError,
    };

    /// A document holding one operation, with `build` applied to it.
    fn with_operation(build: impl FnOnce(Operation) -> Operation) -> Document {
        let mut document = document();
        let template = PathTemplate::parse("/orders").expect("a valid template");
        document.paths.insert(
            &template,
            PathItem::new().with_operation(Method::Get, build(Operation::default())),
        );
        document
    }

    /// A document whose one operation sends `content` as its request body.
    fn with_request_content(content: MediaType) -> Document {
        with_operation(|operation| {
            operation.with_request_body(crate::model::body::RequestBody::new(
                "application/json",
                content,
            ))
        })
    }

    /// The location a media-type construct is reported at.
    fn in_request_body(field: &str) -> String {
        format!("#/paths/~1orders/get/requestBody/content/application~1json/{field}")
    }

    /// A document defining one security scheme under `Guard`.
    fn with_security_scheme(scheme: SecurityScheme) -> Document {
        let mut document = document();
        document.components.insert_security_scheme(
            &ComponentName::new("Guard").expect("a legal component key"),
            scheme,
        );
        document
    }

    /// The location a security-scheme construct is reported at.
    fn in_security_scheme(field: &str) -> String {
        format!("#/components/securitySchemes/Guard/{field}")
    }

    /// An OAuth 2.0 scheme carrying exactly `flows`.
    fn oauth2(flows: OAuthFlows) -> SecurityScheme {
        SecurityScheme::OAuth2 {
            flows: Box::new(flows),
            oauth2_metadata_url: None,
            description: None,
            deprecated: None,
            extensions: crate::model::extensions::Extensions::new(),
        }
    }

    /// One group per `blockers.push` site in `downgrade.rs`, each holding a row
    /// per construct that site reports.
    ///
    /// Grouped rather than flat so the count below means something: two sites
    /// report three fields apiece from a loop, and a flat list could not be
    /// compared against the sites.
    // Long because it is a table, not a procedure: one row per construct the
    // downgrade reports, and the count below is only meaningful if every one is
    // written out here.
    #[expect(clippy::too_many_lines)]
    fn ledger() -> Vec<Vec<(Document, String)>> {
        vec![
            vec![(
                Document {
                    self_uri: Some("https://example.com/orders".to_owned()),
                    ..document()
                },
                "#/$self".to_owned(),
            )],
            vec![(
                Document {
                    servers: vec![Server {
                        name: Some("eu".to_owned()),
                        ..Server::new("https://example.com")
                    }],
                    ..document()
                },
                "#/servers/0/name".to_owned(),
            )],
            vec![
                (
                    Document {
                        tags: vec![Tag {
                            summary: Some("Orders".to_owned()),
                            ..Tag::new("orders")
                        }],
                        ..document()
                    },
                    "#/tags/0/summary".to_owned(),
                ),
                (
                    Document {
                        tags: vec![Tag::new("orders").with_parent("root")],
                        ..document()
                    },
                    "#/tags/0/parent".to_owned(),
                ),
                (
                    Document {
                        tags: vec![Tag::new("orders").with_kind("nav")],
                        ..document()
                    },
                    "#/tags/0/kind".to_owned(),
                ),
            ],
            vec![(
                {
                    let mut document = document();
                    document.components.media_types.insert(
                        "application/json".to_owned(),
                        RefOr::Item(MediaType::new(Schema::any())),
                    );
                    document
                },
                "#/components/mediaTypes".to_owned(),
            )],
            vec![(
                {
                    let mut document = document();
                    let template = PathTemplate::parse("/orders").expect("a valid template");
                    document.paths.insert(
                        &template,
                        PathItem {
                            query: Some(Box::new(Operation::default())),
                            ..PathItem::new()
                        },
                    );
                    document
                },
                "#/paths/~1orders/query".to_owned(),
            )],
            vec![(
                {
                    let mut document = document();
                    let template = PathTemplate::parse("/orders").expect("a valid template");
                    let mut item = PathItem::new();
                    item.additional_operations
                        .insert("PURGE".to_owned(), Box::new(Operation::default()));
                    document.paths.insert(&template, item);
                    document
                },
                "#/paths/~1orders/additionalOperations".to_owned(),
            )],
            vec![(
                with_operation(|operation| {
                    operation.with_parameter(Parameter::new(
                        "filter",
                        ParameterIn::Querystring,
                        Schema::any(),
                    ))
                }),
                "#/paths/~1orders/get/parameters/filter".to_owned(),
            )],
            vec![(
                with_operation(|operation| {
                    operation.with_parameter(
                        Parameter::header("session", Schema::any())
                            .with_style(Style::Cookie, false),
                    )
                }),
                "#/paths/~1orders/get/parameters/session/style".to_owned(),
            )],
            vec![(
                with_operation(|operation| {
                    operation.with_responses(Responses::new().with(
                        200,
                        Response {
                            summary: Some("Created".to_owned()),
                            ..Response::new("ok")
                        },
                    ))
                }),
                "#/paths/~1orders/get/responses/200/summary".to_owned(),
            )],
            vec![
                (
                    with_request_content(MediaType::sequential(Schema::any())),
                    in_request_body("itemSchema"),
                ),
                (
                    with_request_content({
                        let mut content = MediaType::new(Schema::any());
                        content.prefix_encoding = Some(vec![Encoding::new("text/plain")]);
                        content
                    }),
                    in_request_body("prefixEncoding"),
                ),
                (
                    with_request_content({
                        let mut content = MediaType::new(Schema::any());
                        content.item_encoding = Some(Box::new(Encoding::new("text/plain")));
                        content
                    }),
                    in_request_body("itemEncoding"),
                ),
            ],
            // A security scheme's own 3.2 fields. `deprecated` is on all five
            // variants, so one row apiece: a walk that visited only the variant
            // it was written against would pass a single row and still let the
            // other four through.
            vec![
                (
                    with_security_scheme(SecurityScheme::ApiKey {
                        name: "X-Api-Key".to_owned(),
                        location: ParameterIn::Header,
                        description: None,
                        deprecated: Some(true),
                        extensions: crate::model::extensions::Extensions::new(),
                    }),
                    in_security_scheme("deprecated"),
                ),
                (
                    with_security_scheme(SecurityScheme::Http {
                        scheme: "bearer".to_owned(),
                        bearer_format: None,
                        description: None,
                        deprecated: Some(true),
                        extensions: crate::model::extensions::Extensions::new(),
                    }),
                    in_security_scheme("deprecated"),
                ),
                (
                    with_security_scheme(SecurityScheme::MutualTls {
                        description: None,
                        deprecated: Some(true),
                        extensions: crate::model::extensions::Extensions::new(),
                    }),
                    in_security_scheme("deprecated"),
                ),
                (
                    with_security_scheme(SecurityScheme::OpenIdConnect {
                        open_id_connect_url: "https://auth.example.com/.well-known".to_owned(),
                        description: None,
                        deprecated: Some(true),
                        extensions: crate::model::extensions::Extensions::new(),
                    }),
                    in_security_scheme("deprecated"),
                ),
                (
                    with_security_scheme(SecurityScheme::OAuth2 {
                        flows: Box::new(OAuthFlows::default()),
                        oauth2_metadata_url: None,
                        description: None,
                        deprecated: Some(true),
                        extensions: crate::model::extensions::Extensions::new(),
                    }),
                    in_security_scheme("deprecated"),
                ),
            ],
            vec![(
                with_security_scheme(SecurityScheme::OAuth2 {
                    flows: Box::new(OAuthFlows::default()),
                    oauth2_metadata_url: Some(
                        "https://auth.example.com/.well-known/oauth-authorization-server"
                            .to_owned(),
                    ),
                    description: None,
                    deprecated: None,
                    extensions: crate::model::extensions::Extensions::new(),
                }),
                in_security_scheme("oauth2MetadataUrl"),
            )],
            vec![(
                with_security_scheme(oauth2(OAuthFlows {
                    device_authorization: Some(OAuthFlow::new([(
                        "orders:read".to_owned(),
                        "Read orders".to_owned(),
                    )])),
                    ..OAuthFlows::default()
                })),
                in_security_scheme("flows/deviceAuthorization"),
            )],
            // The URL on a flow 3.1 *can* express, so the flow is not the
            // blocker and the field is. Without this row a walk could report
            // the whole `deviceAuthorization` flow and miss the field wherever
            // it rides on one of the four older flows.
            vec![(
                with_security_scheme(oauth2(OAuthFlows {
                    authorization_code: Some(OAuthFlow {
                        device_authorization_url: Some(
                            "https://auth.example.com/device".to_owned(),
                        ),
                        ..OAuthFlow::new([("orders:read".to_owned(), "Read orders".to_owned())])
                    }),
                    ..OAuthFlows::default()
                })),
                in_security_scheme("flows/authorizationCode/deviceAuthorizationUrl"),
            )],
        ]
    }

    #[test]
    fn each_construct_is_reported_at_the_location_it_lives_at() {
        for (document, expected) in ledger().into_iter().flatten() {
            assert_eq!(
                three_two_only_constructs(&document),
                vec![expected],
                "a document using only this construct must report only it"
            );
        }
    }

    /// The blockers reach the caller, rather than only the collector.
    #[test]
    fn a_document_using_one_refuses_to_emit_as_three_one() {
        for (document, expected) in ledger().into_iter().flatten() {
            let error = document
                .emit(SpecVersion::V3_1)
                .expect_err("a 3.2 construct cannot be emitted as 3.1");

            assert_eq!(
                error,
                SpecError::RequiresV3_2 {
                    blockers: vec![expected],
                }
            );
        }
    }

    #[test]
    fn every_construct_has_a_case() {
        const SOURCE: &str = include_str!("downgrade.rs");

        let sites = SOURCE.matches("blockers.push(").count();
        assert_eq!(
            ledger().len(),
            sites,
            "`downgrade.rs` reports from {sites} site(s) and {} are covered; a construct added \
             without a case is one that can stop being detected, which makes a 3.2 document emit \
             as 3.1 with no complaint",
            ledger().len()
        );
    }
}
