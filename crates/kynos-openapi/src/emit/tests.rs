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
    use std::{collections::BTreeSet, fs, path::Path};

    use super::document;
    use crate::{
        emit::downgrade::three_two_only_constructs,
        model::{
            body::{encoding::Encoding, media_type::MediaType},
            components::ComponentName,
            document::{Document, SpecVersion},
            example::Example,
            parameter::{Parameter, ParameterIn, style::Style},
            paths::{item::PathItem, method::Method, operation::Operation, template::PathTemplate},
            reference::RefOr,
            response::{Response, Responses},
            schema::{Schema, discriminator::Discriminator, object::SchemaObject, xml::Xml},
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
            // The Encoding Object's own three fields, which share their names
            // with the three the Media Type Object carries. A walk that stops
            // at the media type sees neither the nested `encoding` map nor the
            // two positional fields beneath it.
            vec![
                (
                    with_request_content(
                        MediaType::new(Schema::any()).with_encoding(
                            "part",
                            Encoding {
                                encoding: [("inner".to_owned(), Encoding::new("text/plain"))]
                                    .into_iter()
                                    .collect(),
                                ..Encoding::new("multipart/mixed")
                            },
                        ),
                    ),
                    in_request_body("encoding/part/encoding"),
                ),
                (
                    with_request_content(MediaType::new(Schema::any()).with_encoding(
                        "part",
                        Encoding {
                            prefix_encoding: Some(vec![Encoding::new("text/plain")]),
                            ..Encoding::new("multipart/mixed")
                        },
                    )),
                    in_request_body("encoding/part/prefixEncoding"),
                ),
                (
                    with_request_content(MediaType::new(Schema::any()).with_encoding(
                        "part",
                        Encoding {
                            item_encoding: Some(Box::new(Encoding::new("text/plain"))),
                            ..Encoding::new("multipart/mixed")
                        },
                    )),
                    in_request_body("encoding/part/itemEncoding"),
                ),
            ],
            // The two example fields 3.2 added beside `value`. `data_external`
            // is its own row because `externalValue` is a field 3.1 *can*
            // express, so the value is not the blocker and `dataValue` riding
            // on it is.
            vec![
                (
                    with_request_content(
                        MediaType::new(Schema::any())
                            .with_named_example("e", Example::data(serde_json::json!({"id": 1}))),
                    ),
                    in_request_body("examples/e/dataValue"),
                ),
                (
                    with_request_content(
                        MediaType::new(Schema::any())
                            .with_named_example("e", Example::serialized("id=1")),
                    ),
                    in_request_body("examples/e/serializedValue"),
                ),
                (
                    with_request_content(MediaType::new(Schema::any()).with_named_example(
                        "e",
                        Example::data_external(
                            serde_json::json!({"id": 1}),
                            "https://example.com/e.json",
                        ),
                    )),
                    in_request_body("examples/e/dataValue"),
                ),
            ],
            // Inside a Schema Object, which the walk did not enter at all.
            // Neither name is an `x-` extension, so 3.1 has no way to read
            // either -- even though 3.1 leaves the schema subtree open enough
            // that its own meta-schema does not object.
            vec![
                (
                    with_request_content(MediaType::new(Schema::Object(Box::new(SchemaObject {
                        xml: Some(Xml {
                            node_type: Some("element".to_owned()),
                            ..Xml::default()
                        }),
                        ..SchemaObject::default()
                    })))),
                    in_request_body("schema/xml/nodeType"),
                ),
                (
                    with_request_content(MediaType::new(Schema::Object(Box::new(SchemaObject {
                        discriminator: Some(Discriminator {
                            default_mapping: Some("#/components/schemas/Fallback".to_owned()),
                            ..Discriminator::new("kind")
                        }),
                        ..SchemaObject::default()
                    })))),
                    in_request_body("schema/discriminator/defaultMapping"),
                ),
            ],
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

    /// Every wire name the model gates behind `openapi32`, read from `model/`.
    ///
    /// A field is a gate followed, past any attributes and documentation, by a
    /// field declaration; its wire name is its `rename` where it has one and
    /// its identifier otherwise. Gates introducing an enum variant, a match
    /// arm, an `impl` or a function are skipped -- they are not wire names.
    fn gated_wire_names() -> BTreeSet<String> {
        const GATE: &str = "#[cfg(feature = \"openapi32\")]";

        fn walk(directory: &Path, names: &mut BTreeSet<String>) {
            let entries = fs::read_dir(directory).expect("the model sources are beside this test");
            for entry in entries.map(|entry| entry.expect("a readable directory entry")) {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, names);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    collect(
                        &fs::read_to_string(&path).expect("a readable source file"),
                        names,
                    );
                }
            }
        }

        fn collect(source: &str, names: &mut BTreeSet<String>) {
            let lines: Vec<&str> = source.lines().map(str::trim).collect();
            for (index, line) in lines.iter().enumerate() {
                if *line != GATE {
                    continue;
                }

                // Step past the attributes and documentation between the gate
                // and whatever it gates, tracking brackets so a `#[serde(..)]`
                // written over several lines is one step rather than several.
                let mut cursor = index + 1;
                let mut depth = 0i32;
                while let Some(current) = lines.get(cursor) {
                    let open = current.matches('[').count() + current.matches('(').count();
                    let close = current.matches(']').count() + current.matches(')').count();
                    let attribute = depth > 0 || current.starts_with("#[");
                    if !attribute && !current.starts_with("//") && !current.is_empty() {
                        break;
                    }
                    depth += i32::try_from(open).expect("a short line")
                        - i32::try_from(close).expect("a short line");
                    cursor += 1;
                }

                let Some(declaration) = lines.get(cursor) else {
                    continue;
                };
                let Some(identifier) = field_identifier(declaration) else {
                    continue;
                };

                let attributes = lines[index + 1..cursor].join(" ");
                names.insert(rename_in(&attributes).unwrap_or_else(|| camel_case(&identifier)));
            }
        }

        /// The identifier of `line` when it *declares* a field.
        ///
        /// Nothing for a variant, an arm, or any other item a gate may sit
        /// above -- and nothing for a field *initializer*, which shares the
        /// `name: rest` shape. The two are told apart by what follows the
        /// colon: a declaration names a type, an initializer gives a value.
        fn field_identifier(line: &str) -> Option<String> {
            let declaration = line.strip_prefix("pub ").unwrap_or(line);
            let (identifier, rest) = declaration.split_once(": ")?;
            let identifier = identifier.trim();

            let names_a_type = rest.starts_with(|c: char| c.is_ascii_uppercase())
                || ["bool", "u16", "u32", "u64", "f64", "usize", "String"]
                    .iter()
                    .any(|primitive| rest.starts_with(primitive));
            let names_a_field = !identifier.is_empty()
                && identifier.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && identifier
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_');

            (names_a_type && names_a_field && line.ends_with(',')).then(|| identifier.to_owned())
        }

        /// A field identifier as the wire spells it when no `rename` says
        /// otherwise. Every renamed field in this model is renamed to the
        /// camelCase of its identifier, and a single-word identifier is its own
        /// camelCase -- so this is the same rule read from the other side, and
        /// it is what lets the domain struct and the wire struct that carries
        /// the `rename` agree on one name.
        fn camel_case(identifier: &str) -> String {
            let mut camel = String::with_capacity(identifier.len());
            let mut capitalize = false;
            for character in identifier.chars() {
                if character == '_' {
                    capitalize = true;
                } else if capitalize {
                    camel.extend(character.to_uppercase());
                    capitalize = false;
                } else {
                    camel.push(character);
                }
            }
            camel
        }

        /// The `rename = "..."` value within a run of attributes.
        fn rename_in(attributes: &str) -> Option<String> {
            let (_, rest) = attributes.split_once("rename = \"")?;
            let (value, _) = rest.split_once('"')?;
            Some(value.to_owned())
        }

        let mut names = BTreeSet::new();
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/model"),
            &mut names,
        );
        assert!(
            !names.is_empty(),
            "the scan found no gated field, so it is measuring nothing"
        );
        names
    }

    /// Every 3.2 field the model can hold is one the downgrade reports.
    ///
    /// The anchor here is the *model*, and that is the whole point. This test
    /// counted `blockers.push` sites in `downgrade.rs` against the ledger,
    /// which proves every blocker has a case -- the converse of what has to
    /// hold. A field added with no blocker is exactly the document that emits
    /// as 3.1 while carrying a keyword 3.1 cannot read, and a reporter counted
    /// against itself can never see one. Five fields had slipped through when
    /// this was written.
    ///
    /// Names are matched against the string literals `downgrade.rs` builds its
    /// locations from, rather than against its source text, so an identifier
    /// that merely happens to share a field's spelling does not satisfy it.
    #[test]
    fn every_three_two_field_is_reported() {
        const REPORTER: &str = include_str!("downgrade.rs");

        let literals: String = REPORTER
            .split('"')
            .skip(1)
            .step_by(2)
            .collect::<Vec<_>>()
            .join("\u{1f}");

        let unreported: Vec<String> = gated_wire_names()
            .into_iter()
            .filter(|name| !literals.contains(name.as_str()))
            .collect();

        assert!(
            unreported.is_empty(),
            "the model gates {unreported:?} behind `openapi32` and the downgrade names \
             none of them, so a document carrying one emits as 3.1 with no complaint"
        );
    }

    /// Every construct the downgrade reports is exercised by a ledger row.
    ///
    /// The counterpart to the test above, and the weaker direction: it keeps a
    /// reporting site from losing the case that proves what it says.
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
