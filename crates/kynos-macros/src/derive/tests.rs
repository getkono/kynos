//! The ledger: one case per diagnostic the derives raise.
//!
//! Each derive is an attribute grammar over a closed key set, so what it owes
//! is a case per rule rather than a generator that would re-derive its match
//! arms — see `docs/testing.md`. The rows here assert *which* diagnostic fired;
//! the wording is held by the snapshots in `crates/kynos/tests/ui/macros/`,
//! where a reader sees it rendered.
//!
//! One ledger rather than one test module per derive, because the counters are
//! the point and they are the same counter six times over. A rule added to any
//! grammar without a row fails the build.

use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;

/// What was written, and a fragment of what the derive must say about it.
struct Case {
    description: &'static str,
    input: DeriveInput,
    expects: &'static str,
}

fn case(description: &'static str, declaration: TokenStream2, expects: &'static str) -> Case {
    Case {
        description,
        input: syn::parse2(declaration).expect("the case itself must parse"),
        expects,
    }
}

/// Runs a ledger against the expansion that owns it.
fn each_case_is_refused(ledger: Vec<Case>, expand: fn(&DeriveInput) -> syn::Result<TokenStream2>) {
    for Case {
        description,
        input,
        expects,
    } in ledger
    {
        let Err(error) = expand(&input) else {
            panic!("{description} must be rejected");
        };
        let reported = error.to_string();
        assert!(
            reported.contains(expects),
            "{description}: expected a diagnostic containing {expects:?}, got {reported:?}"
        );
    }
}

/// Counts a ledger against the diagnostic sites of the file it covers.
///
/// A count, not a mapping: it catches the drift that happens — a rule added
/// without a case — and not a row rewritten to reach a site another covers.
fn every_diagnostic_has_a_case(file: &str, source: &str, cases: usize) {
    let sites = source.matches("syn::Error::new(").count() + source.matches("meta.error(").count();
    assert_eq!(
        cases, sites,
        "`{file}` raises {sites} diagnostic(s) and {cases} have a case; a grammar rule added \
         without one is a rule that can stop firing silently"
    );
}

mod schema {
    use super::{
        Case, DeriveInput, TokenStream2, case, each_case_is_refused, every_diagnostic_has_a_case,
    };
    use crate::derive::schema::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a union, which no JSON value corresponds to",
                quote::quote!(
                    union Payload {
                        a: u32,
                    }
                ),
                "cannot describe a union",
            ),
            case(
                "`format`, which states what a value is rather than constraining it",
                quote::quote!(
                    struct Order {
                        #[schema(format = "uuid")]
                        id: String,
                    }
                ),
                "`format` says what a value",
            ),
            case(
                "`unique_items` given a value, when it is a flag",
                quote::quote!(
                    struct Order {
                        #[schema(unique_items = 1)]
                        tags: Vec<String>,
                    }
                ),
                "is a flag",
            ),
            case(
                "a numeric constraint given a string",
                quote::quote!(
                    struct Order {
                        #[schema(minimum = "x")]
                        total: u32,
                    }
                ),
                "takes a number",
            ),
            case(
                "a count constraint given a string",
                quote::quote!(
                    struct Order {
                        #[schema(min_length = "x")]
                        name: String,
                    }
                ),
                "takes a non-negative whole number",
            ),
            case(
                "a key outside the constraint grammar",
                quote::quote!(
                    struct Order {
                        #[schema(nonsense = 1)]
                        total: u32,
                    }
                ),
                "is not part of the `#[schema(...)]` grammar",
            ),
            case(
                "`#[schema(open)]` on a second flattened field of one container",
                quote::quote!(
                    struct Thing {
                        #[serde(flatten)]
                        #[schema(open)]
                        extra: BTreeMap<String, String>,
                        #[serde(flatten)]
                        #[schema(open)]
                        more: BTreeMap<String, String>,
                    }
                ),
                "may appear once per container",
            ),
            case(
                "`#[schema(open)]` on a field that is not flattened",
                quote::quote!(
                    struct Thing {
                        #[schema(open)]
                        extra: BTreeMap<String, String>,
                    }
                ),
                "only a flattened field has anything",
            ),
        ]
    }

    /// The serde attributes whose wire form the schema could not follow.
    ///
    /// A second function rather than more rows in the first, for the reason
    /// `security_scheme`'s `oauth2_ledger` gives: one list of every diagnostic
    /// had outgrown what Clippy will accept. These are the refusals the
    /// derive's rustdoc lists as serde and the schema disagreeing.
    fn serde_ledger() -> Vec<Case> {
        vec![
            case(
                "an untagged enum, which has no describable decoding rule",
                quote::quote!(
                    #[serde(untagged)]
                    enum Payload {
                        Number(u32),
                        Text(String),
                    }
                ),
                "an untagged enum",
            ),
            case(
                "`serialize_with` on a field, whose wire form its type no longer predicts",
                quote::quote!(
                    struct Reading {
                        #[serde(serialize_with = "as_string")]
                        count: u64,
                    }
                ),
                "does not predict",
            ),
            case(
                "a `#[serde(other)]` catch-all, which only 3.2's `defaultMapping` could describe",
                quote::quote!(
                    #[serde(tag = "kind")]
                    enum Event {
                        Created {
                            id: u64,
                        },
                        #[serde(other)]
                        Unknown,
                    }
                ),
                "`#[serde(other)]` accepts",
            ),
            case(
                "`skip_serializing_if` on a field serde still requires on read",
                quote::quote!(
                    struct Draft {
                        #[serde(skip_serializing_if = "String::is_empty")]
                        elided: String,
                    }
                ),
                "still requires it on read",
            ),
            case(
                "`skip_serializing_if` on a flattened field that is not an open map",
                quote::quote!(
                    struct Wrapper {
                        id: u64,
                        #[serde(flatten, skip_serializing_if = "Audit::is_empty")]
                        audit: Audit,
                    }
                ),
                "on a flattened field is refused unless it is `#[schema(open)]`",
            ),
            case(
                "`into` and `from` on a struct, which serde writes and reads as another type",
                quote::quote!(
                    #[serde(into = "String", from = "String")]
                    struct Celsius {
                        degrees: i64,
                    }
                ),
                "as the type it names",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(serde_ledger(), expand_inner);
    }

    #[test]
    fn every_schema_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "schema.rs",
            include_str!("schema.rs"),
            ledger().len() + serde_ledger().len(),
        );
    }

    /// `#[serde(untagged)]` on a struct is serde's diagnostic to raise, not ours.
    ///
    /// The refusal exists because an untagged *enum* has no describable
    /// decoding rule. A struct has no variants to choose between, so the
    /// sentence does not apply to one -- and serde already refuses the
    /// attribute there, in its own words. Raising a second diagnostic that
    /// calls a struct an enum is this derive restating a serde shape rule and
    /// getting the noun wrong, which is exactly what `Container` reads serde's
    /// attributes rather than re-deriving them in order to avoid.
    #[test]
    fn untagged_on_a_struct_is_left_to_serde() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(untagged)]
            struct Receipt {
                total: u32,
            }
        ))
        .expect("the case itself must parse");

        let Err(error) = expand_inner(&input) else {
            return;
        };

        assert!(
            !error.to_string().contains("untagged enum"),
            "a struct was refused with a sentence about enums: {error}"
        );
    }

    /// Each of serde's three wire-form overrides is refused wherever serde
    /// accepts it, on a field and on a variant alike.
    ///
    /// One row per key and placement, each written out: the ledger's single row
    /// proves the site fires, and this proves the scan reaches every key and
    /// names the one that was written.
    #[test]
    fn every_wire_form_override_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`with` on a field",
                    quote::quote!(
                        struct Reading {
                            #[serde(with = "as_string")]
                            count: u64,
                        }
                    ),
                    "`with` reads or writes this field",
                ),
                case(
                    "`serialize_with` on a field",
                    quote::quote!(
                        struct Reading {
                            #[serde(serialize_with = "as_string")]
                            count: u64,
                        }
                    ),
                    "`serialize_with` reads or writes this field",
                ),
                case(
                    "`deserialize_with` on a field",
                    quote::quote!(
                        struct Reading {
                            #[serde(deserialize_with = "from_string")]
                            count: u64,
                        }
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
                case(
                    "`with` on a variant",
                    quote::quote!(
                        enum Reading {
                            #[serde(with = "as_string")]
                            Count(u64),
                        }
                    ),
                    "`with` reads or writes this variant",
                ),
                case(
                    "`serialize_with` on a variant",
                    quote::quote!(
                        enum Reading {
                            #[serde(serialize_with = "as_string")]
                            Count(u64),
                        }
                    ),
                    "`serialize_with` reads or writes this variant",
                ),
                case(
                    "`deserialize_with` on a variant",
                    quote::quote!(
                        enum Reading {
                            #[serde(deserialize_with = "from_string")]
                            Count(u64),
                        }
                    ),
                    "`deserialize_with` reads or writes this variant",
                ),
                // The tuple, newtype and tuple-variant shapes emit every member
                // whatever its skip attributes say, so a skipped member there
                // is still described and its override still contradicts it.
                case(
                    "`serialize_with` on a skipped member of a tuple struct",
                    quote::quote!(
                        struct Pair(
                            u64,
                            #[serde(skip_deserializing, serialize_with = "as_string")] u64,
                        );
                    ),
                    "`serialize_with` reads or writes this field",
                ),
                case(
                    "`serialize_with` on the skipped member of a newtype",
                    quote::quote!(
                        struct Sku(#[serde(skip_deserializing, serialize_with = "as_string")] u64);
                    ),
                    "`serialize_with` reads or writes this field",
                ),
                case(
                    "`serialize_with` on a skipped member of a tuple variant",
                    quote::quote!(
                        enum Reading {
                            Count(
                                u64,
                                #[serde(skip_deserializing, serialize_with = "as_string")] u64,
                            ),
                        }
                    ),
                    "`serialize_with` reads or writes this field",
                ),
            ],
            expand_inner,
        );
    }

    /// A wire-form override on something no schema describes is left alone.
    ///
    /// The refusal exists because the schema would describe a value the wire
    /// never carries. A skipped field, and every field of a skipped variant,
    /// are in no schema at all, so there is nothing for the override to
    /// contradict.
    #[test]
    fn a_wire_form_override_on_an_undescribed_field_is_left_alone() {
        for declaration in [
            quote::quote!(
                struct Reading {
                    total: u64,
                    #[serde(skip, with = "as_string")]
                    count: u64,
                }
            ),
            quote::quote!(
                #[serde(tag = "kind")]
                enum Reading {
                    Total {
                        total: u64,
                    },
                    #[serde(skip)]
                    Count {
                        #[serde(with = "as_string")]
                        count: u64,
                    },
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            // Expansion must succeed outright: checking only that the error is
            // not this refusal would pass for a declaration refused for any
            // other reason.
            if let Err(error) = expand_inner(&input) {
                panic!("an override nothing describes must expand, and was refused: {error}");
            }
        }
    }

    /// Each of serde's three container conversions is refused on a struct and
    /// on an enum alike.
    ///
    /// One row per key and shape, each written out: the ledger's single row
    /// proves the site fires, and this proves the scan reaches every key, both
    /// shapes, and names the first key written rather than the first one it
    /// looks for.
    #[test]
    fn every_container_conversion_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`into` on a struct",
                    quote::quote!(
                        #[serde(into = "String")]
                        struct Celsius {
                            degrees: i64,
                        }
                    ),
                    "`into` makes serde read or write this struct as the type it names",
                ),
                case(
                    "`from` on a struct",
                    quote::quote!(
                        #[serde(from = "String")]
                        struct Celsius {
                            degrees: i64,
                        }
                    ),
                    "`from` makes serde read or write this struct as the type it names",
                ),
                case(
                    "`try_from` on a struct",
                    quote::quote!(
                        #[serde(try_from = "String")]
                        struct Even {
                            value: u64,
                        }
                    ),
                    "`try_from` makes serde read or write this struct as the type it names",
                ),
                case(
                    "`into` on an internally tagged enum",
                    quote::quote!(
                        #[serde(tag = "kind", into = "String")]
                        enum Speed {
                            Fast,
                            Slow,
                        }
                    ),
                    "`into` makes serde read or write this enum as the type it names",
                ),
                case(
                    "`from` written before `into`, in separate attributes",
                    quote::quote!(
                        #[serde(from = "String")]
                        #[serde(into = "String")]
                        struct Celsius {
                            degrees: i64,
                        }
                    ),
                    "`from` makes serde read or write this struct as the type it names",
                ),
            ],
            expand_inner,
        );
    }

    /// `#[serde(remote = ...)]` is left alone.
    ///
    /// It derives serde's traits for the type it names as inherent functions on
    /// this one, whose fields mirror that type's, so the declaration still
    /// predicts the wire form and there is no disagreement to refuse.
    #[test]
    fn a_remote_container_is_left_alone() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(remote = "Duration")]
            struct DurationDef {
                secs: u64,
                nanos: u32,
            }
        ))
        .expect("the case itself must parse");

        // Expansion must succeed outright, for the reason
        // `a_wire_form_override_on_an_undescribed_field_is_left_alone` gives.
        if let Err(error) = expand_inner(&input) {
            panic!("a remote container must expand, and was refused: {error}");
        }
    }

    /// A catch-all the schema skips is refused all the same.
    ///
    /// Unlike a wire-form override, `#[serde(other)]` is not about the
    /// variant's own branch: `skip_serializing` keeps that branch out of the
    /// schema, but deserialization still routes every tag the enum does not
    /// name to it, so the schema's closed `oneOf` still disagrees with what the
    /// type accepts.
    #[test]
    fn a_catch_all_on_a_skipped_variant_is_still_refused() {
        each_case_is_refused(
            vec![case(
                "a `#[serde(other)]` catch-all that is never serialized",
                quote::quote!(
                    #[serde(tag = "kind")]
                    enum Event {
                        Created {
                            id: u64,
                        },
                        #[serde(skip_serializing)]
                        #[serde(other)]
                        Unknown,
                    }
                ),
                "`#[serde(other)]` accepts",
            )],
            expand_inner,
        );
    }

    /// `skip_serializing_if` is accepted wherever serde may leave the field out
    /// in both directions, or never reads it at all.
    ///
    /// Beside an `Option` or a `#[serde(default)]`, an absent field reads as
    /// well as it writes, so `required` can leave it out truthfully. A field
    /// that is never read is in no schema, so nothing can disagree with it.
    #[test]
    fn skip_serializing_if_beside_an_option_or_a_default_is_accepted() {
        for declaration in [
            quote::quote!(
                struct Draft {
                    #[serde(skip_serializing_if = "Option::is_none")]
                    maybe: Option<u64>,
                }
            ),
            quote::quote!(
                struct Draft {
                    #[serde(default, skip_serializing_if = "String::is_empty")]
                    elided: String,
                }
            ),
            quote::quote!(
                struct Draft {
                    #[serde(skip_deserializing, skip_serializing_if = "String::is_empty")]
                    elided: String,
                }
            ),
            // A container `default` fills every missing field from `Default`
            // on read, so each field is as absent-tolerant as a field-level
            // `default` would make it.
            quote::quote!(
                #[serde(default)]
                struct Draft {
                    #[serde(skip_serializing_if = "String::is_empty")]
                    elided: String,
                }
            ),
            // A flattened open map: serde reads its absence as an empty map,
            // and no flattened field is ever listed in `required`.
            quote::quote!(
                struct Draft {
                    id: u64,
                    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
                    #[schema(open)]
                    extra: HashMap<String, String>,
                }
            ),
            // The same open map with a default, which a flattened field is
            // decided without: `#[schema(open)]` alone accepts it.
            quote::quote!(
                struct Draft {
                    id: u64,
                    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
                    #[schema(open)]
                    extra: HashMap<String, String>,
                }
            ),
            // The same rule inside an internally tagged struct variant: an
            // `Option` field may be absent both ways.
            quote::quote!(
                #[serde(tag = "kind")]
                enum Event {
                    Created {
                        #[serde(skip_serializing_if = "Option::is_none")]
                        note: Option<String>,
                    },
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            if let Err(error) = expand_inner(&input) {
                panic!("a field serde may omit in both directions must expand: {error}");
            }
        }
    }

    /// A flattened field that skips itself on write is refused unless it is
    /// `#[schema(open)]`, whatever default it or its struct carries.
    ///
    /// The refusal reads attributes, not types, so a map and a struct meet the
    /// same site. A flattened struct is written whole or not at all, and serde
    /// ignores `#[serde(default)]` on a flattened field at field and container
    /// level alike, so no default covers it. A flattened map is decided by
    /// `#[schema(open)]` alone, which the map row below leaves off.
    #[test]
    fn a_flattened_field_skipped_on_write_is_refused_unless_open() {
        each_case_is_refused(
            vec![
                case(
                    "`skip_serializing_if` on a flattened struct with no default",
                    quote::quote!(
                        struct Draft {
                            id: u64,
                            #[serde(flatten, skip_serializing_if = "Audit::is_empty")]
                            audit: Audit,
                        }
                    ),
                    "on a flattened field is refused unless it is `#[schema(open)]`",
                ),
                case(
                    "`skip_serializing_if` on a flattened struct with a field-level default",
                    quote::quote!(
                        struct Wrapper {
                            id: u64,
                            #[serde(flatten, default, skip_serializing_if = "Audit::is_empty")]
                            audit: Audit,
                        }
                    ),
                    "on a flattened field is refused unless it is `#[schema(open)]`",
                ),
                case(
                    "`skip_serializing_if` on a flattened struct under a container default",
                    quote::quote!(
                        #[serde(default)]
                        struct Wrapper {
                            id: u64,
                            #[serde(flatten, skip_serializing_if = "Audit::is_empty")]
                            audit: Audit,
                        }
                    ),
                    "on a flattened field is refused unless it is `#[schema(open)]`",
                ),
                case(
                    "`skip_serializing_if` on a flattened map that is not `#[schema(open)]`",
                    quote::quote!(
                        struct Tagged {
                            id: u64,
                            #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
                            extra: HashMap<String, String>,
                        }
                    ),
                    "on a flattened field is refused unless it is `#[schema(open)]`",
                ),
            ],
            expand_inner,
        );
    }

    /// `skip_serializing_if` on a field of an enum variant follows the rule a
    /// struct's field does.
    ///
    /// The refusal walks every variant serde writes, because an internally
    /// tagged struct variant is an object with its own `required` list.
    #[test]
    fn a_variant_field_skipped_on_write_is_refused_like_a_struct_field() {
        each_case_is_refused(
            vec![case(
                "`skip_serializing_if` on a non-`Option` variant field with no default",
                quote::quote!(
                    #[serde(tag = "kind")]
                    enum Event {
                        Created {
                            #[serde(skip_serializing_if = "String::is_empty")]
                            note: String,
                        },
                    }
                ),
                "still requires it on read",
            )],
            expand_inner,
        );
    }

    /// Whether the expansion claims `kynos::schema::Flatten` for the input.
    ///
    /// Read off the emitted tokens rather than by calling the predicate, so
    /// what is asserted is the implementation a user receives. `to_string` on a
    /// `TokenStream` separates every token with a space, which is why the
    /// needle is spelt out that way.
    fn claims_flatten(declaration: TokenStream2) -> bool {
        let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
        let expansion = expand_inner(&input).expect("the case itself must expand");
        expansion
            .to_string()
            .contains(":: kynos :: schema :: Flatten for")
    }

    /// The shapes whose description is an object naming its own members.
    ///
    /// A closed enumeration, and the one this change introduced: `Flatten` is
    /// a claim, so a shape that gets the implementation without naming its
    /// members is a lie the compiler then trusts. Each arm of the decision is
    /// asserted from both sides, because a predicate that returned `true`
    /// everywhere would pass every positive case on its own.
    #[test]
    fn only_a_shape_naming_its_members_claims_flatten() {
        // A struct: named fields name them, and no other shape does.
        assert!(claims_flatten(quote::quote!(
            struct Audit {
                at: String,
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            struct Sku(String);
        )));
        assert!(!claims_flatten(quote::quote!(
            struct Span(u32, u32);
        )));
        assert!(!claims_flatten(quote::quote!(
            struct Marker;
        )));

        // Adjacently tagged: every branch is an object of a tag property and a
        // content property, whatever the variant holds.
        assert!(claims_flatten(quote::quote!(
            #[serde(tag = "kind", content = "value")]
            enum Payload {
                Number(u32),
                Named { width: u32 },
                Nothing,
            }
        )));

        // Internally tagged: a named or unit variant becomes an object naming
        // its own members plus the tag. A newtype variant composes with
        // whatever its payload resolves to, which is the unknown the trait
        // exists to refuse.
        assert!(claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Shape {
                Circle { radius: f64 },
                Point,
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Shape {
                Circle { radius: f64 },
                Raw(String),
            }
        )));

        // Externally tagged: the variant's name is the single property, and a
        // unit variant is that name as a bare string instead.
        assert!(claims_flatten(quote::quote!(
            enum Event {
                Created { at: String },
                Renamed(String),
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            enum Event {
                Created { at: String },
                Deleted,
            }
        )));
        // Every variant a unit is the compact `enum` of names, which is a
        // string schema and not an object at all.
        assert!(!claims_flatten(quote::quote!(
            enum Currency {
                Gbp,
                Jpy,
            }
        )));

        // A skipped variant reaches no branch, so it cannot disqualify one.
        assert!(claims_flatten(quote::quote!(
            enum Event {
                Created {
                    at: String,
                },
                #[serde(skip)]
                Internal,
            }
        )));

        // An enum with no branch at all describes nothing to flatten.
        assert!(!claims_flatten(quote::quote!(
            enum Never {}
        )));
        assert!(!claims_flatten(quote::quote!(
            #[serde(tag = "kind", content = "value")]
            enum Never {}
        )));
        assert!(!claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Never {}
        )));
    }

    /// A container that is itself open does not claim to be flattenable.
    ///
    /// The subtle one, and the defect re-entering by the back door: an open
    /// container carries `unevaluatedProperties` of its own, and one level up
    /// that keyword sits inside an `allOf` branch where it reaches the outer
    /// object's own properties -- exactly what this change exists to stop.
    #[test]
    fn an_open_container_does_not_claim_flatten() {
        // The same shape without the attribute does claim it, so the case
        // isolates the attribute rather than the shape.
        assert!(claims_flatten(quote::quote!(
            struct Thing {
                id: u64,
                #[serde(flatten)]
                audit: Audit,
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            struct Thing {
                id: u64,
                #[serde(flatten)]
                #[schema(open)]
                extra: BTreeMap<String, String>,
            }
        )));
        // And through a variant, which is a field group like any other.
        assert!(!claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Shape {
                Circle {
                    radius: f64,
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                },
            }
        )));
    }

    /// A transparent struct does not claim to be flattenable.
    ///
    /// serde writes a `#[serde(transparent)]` struct as its one field's value,
    /// so what flattening it contributes is that field's members, not the
    /// struct's. Named fields are the shape of the declaration and say nothing
    /// about the wire, and a transparent wrapper over a map would otherwise
    /// carry the map straight past the bound.
    #[test]
    fn a_transparent_struct_does_not_claim_flatten() {
        // The same declaration without the attribute does claim it, so the
        // case isolates the attribute rather than the shape.
        assert!(claims_flatten(quote::quote!(
            struct Labels {
                inner: BTreeMap<String, String>,
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            #[serde(transparent)]
            struct Labels {
                inner: BTreeMap<String, String>,
            }
        )));
    }

    /// A skipped variant cannot cost an enum its claim to Flatten.
    ///
    /// serde never writes a `#[serde(skip)]` variant, so the derive describes no
    /// branch for it and nothing its fields declare reaches the schema. An open
    /// field inside one is therefore not an open member of the enum.
    #[test]
    fn a_skipped_variant_does_not_disqualify_the_enum() {
        // The same variant unskipped does disqualify it, so the case isolates
        // the skip rather than the shape.
        assert!(!claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Event {
                Created {
                    at: String,
                },
                Internal {
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                },
            }
        )));
        assert!(claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Event {
                Created {
                    at: String,
                },
                #[serde(skip)]
                Internal {
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                },
            }
        )));
    }
}

mod api_error {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::api_error::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a union",
                quote::quote!(
                    union StoreError {
                        a: u32,
                    }
                ),
                "cannot describe a union",
            ),
            case(
                "a status on the enum, where variants answer with their own",
                quote::quote!(
                    #[problem(status = 404)]
                    enum StoreError {
                        #[problem(status = 404)]
                        NotFound,
                    }
                ),
                "a status belongs on each variant",
            ),
            case(
                "a variant that never says what status it produces",
                quote::quote!(
                    enum StoreError {
                        NotFound,
                    }
                ),
                "does not say what status it produces",
            ),
            case(
                "a struct that never says what status it produces",
                quote::quote!(
                    struct StoreError;
                ),
                "does not say what status it produces",
            ),
            case(
                "a status outside the range a problem detail may carry",
                quote::quote!(
                    #[problem(status = 200)]
                    struct StoreError;
                ),
                "its status is between",
            ),
            case(
                "two statuses on one error, when a response has one",
                quote::quote!(
                    #[problem(status = 404, status = 410)]
                    struct StoreError;
                ),
                "already declares a status",
            ),
            case(
                "`base` on a variant, when it is the prefix the type shares",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 404, base = "https://example.com/")]
                        NotFound,
                    }
                ),
                "belongs on the type",
            ),
            case(
                "`extension` on the type, when it marks a field",
                quote::quote!(
                    #[problem(status = 404, extension)]
                    struct StoreError;
                ),
                "belongs on a field",
            ),
            case(
                "a key outside the problem grammar",
                quote::quote!(
                    #[problem(status = 404, titel = "User not found")]
                    struct StoreError;
                ),
                "is not part of the `#[problem(...)]` grammar",
            ),
            case(
                "an extension on a field with no name to publish it under",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 404)]
                        NotFound(#[problem(extension)] u64),
                    }
                ),
                "published under its field's name",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_api_error_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("api_error.rs", include_str!("api_error.rs"), ledger().len());
    }

    /// Every URI a declaration resolves reaches the emitted responses.
    ///
    /// What is checked here is *resolution*: an explicit `type`, a slug hung
    /// under `base`, and two variants sharing a status. Whether the narrowing
    /// is well-formed is `crates/kynos/tests/derives.rs`'s, which reads the
    /// emitted schema; this reads the tokens, so it names the failing URI
    /// without a compile.
    ///
    /// It deliberately does not assert *which* helper the expansion calls. A
    /// path is not a behaviour, and pinning one here would redden this test
    /// for a move that no document could observe.
    #[test]
    fn the_declared_type_reaches_the_emitted_responses() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[problem(base = "https://errors.example.com/")]
            enum StoreError {
                #[problem(status = 404, title = "User not found")]
                NotFound,

                #[problem(status = 404)]
                TenantMissing,

                #[problem(status = 409, type = "https://errors.example.com/email-taken")]
                Conflict,
            }
        ))
        .expect("the case itself must parse");

        let expansion = expand_inner(&input)
            .expect("a well-formed declaration expands")
            .to_string();

        for uri in [
            "https://errors.example.com/not-found",
            "https://errors.example.com/tenant-missing",
            "https://errors.example.com/email-taken",
        ] {
            assert!(
                expansion.contains(uri),
                "`{uri}` never reached the emitted responses: {expansion}"
            );
        }
    }
}

mod reply {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::reply::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a struct, when a reply is a closed set of responses",
                quote::quote!(
                    struct CreateReply;
                ),
                "closed set of responses",
            ),
            case(
                "a union",
                quote::quote!(
                    union CreateReply {
                        a: u32,
                    }
                ),
                "needs an enum",
            ),
            case(
                "a status on the enum, where variants answer with their own",
                quote::quote!(
                    #[reply(status = 200)]
                    enum CreateReply {
                        #[reply(status = 200)]
                        Ok(u32),
                    }
                ),
                "a status belongs on each variant",
            ),
            case(
                "a variant that never says what status it produces",
                quote::quote!(
                    enum CreateReply {
                        Ok(u32),
                    }
                ),
                "does not say what status it produces",
            ),
            case(
                "two variants answering with one status",
                quote::quote!(
                    enum UploadReply {
                        #[reply(status = 202)]
                        Queued(u32),
                        #[reply(status = 202)]
                        AlreadyQueued(u32),
                    }
                ),
                "already answers with",
            ),
            case(
                "a status outside the range a handler may answer with",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 99)]
                        Ok(u32),
                    }
                ),
                "its status is between",
            ),
            case(
                "two statuses on one variant",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 200, status = 201)]
                        Ok(u32),
                    }
                ),
                "already declares a status",
            ),
            case(
                "a key outside the reply grammar",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 200, nonsense = "x")]
                        Ok(u32),
                    }
                ),
                "is not part of the `#[reply(...)]` grammar",
            ),
            case(
                "a struct variant, when a body is one described type",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 201)]
                        Created { id: u32, revision: u32 },
                    }
                ),
                "carries its response body",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_reply_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("reply.rs", include_str!("reply.rs"), ledger().len());
    }
}

mod security_scheme {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::security_scheme::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a scheme that never says what kind it is",
                quote::quote!(
                    struct Bearer;
                ),
                "must say what kind it is",
            ),
            case(
                "two kinds, when a scheme has exactly one",
                quote::quote!(
                    #[security(bearer, basic)]
                    struct Bearer;
                ),
                "exactly one kind",
            ),
            case(
                "a key outside the security grammar",
                quote::quote!(
                    #[security(nonsense)]
                    struct Bearer;
                ),
                "is not part of the `#[security(...)]` grammar",
            ),
            case(
                "an API key that never says where it travels",
                quote::quote!(
                    #[security(api_key(name = "X-Api-Key"))]
                    struct ApiKey;
                ),
                "must say where it travels",
            ),
            case(
                "an API key travelling somewhere it cannot",
                quote::quote!(
                    #[security(api_key(in = "path", name = "key"))]
                    struct ApiKey;
                ),
                "not `path`",
            ),
            case(
                "an API key that never says which field carries it",
                quote::quote!(
                    #[security(api_key(in = "header"))]
                    struct ApiKey;
                ),
                "must say which field carries it",
            ),
            case(
                "an API key claiming a header the specification reserves",
                quote::quote!(
                    #[security(api_key(in = "header", name = "authorization"))]
                    struct ApiKey;
                ),
                "must not be declared as a parameter",
            ),
        ]
    }

    /// The refusals the `oauth2` flow grammar adds.
    ///
    /// A second function rather than more rows in the first: the two are read
    /// together everywhere below, and one list of every diagnostic this derive
    /// raises had outgrown what a reader can hold — and what Clippy will accept.
    fn oauth2_ledger() -> Vec<Case> {
        vec![
            case(
                "an OAuth 2.0 scheme declaring no flow at all",
                quote::quote!(
                    #[security(oauth2(metadata_url = "https://auth.example.com/meta"))]
                    struct Delegated;
                ),
                "must declare at least one flow",
            ),
            case(
                "a flow OAuth 2.0 does not define",
                quote::quote!(
                    #[security(oauth2(magic_link(token_url = "https://auth.example.com/token")))]
                    struct Delegated;
                ),
                "is not an OAuth 2.0 flow",
            ),
            case(
                "a flow missing a URL its own grant needs",
                quote::quote!(
                    #[security(oauth2(authorization_code(
                        token_url = "https://auth.example.com/token"
                    )))]
                    struct Delegated;
                ),
                "authorization_url",
            ),
            case(
                "a carrier setting that is not the one word it takes",
                quote::quote!(
                    #[security(bearer)]
                    #[security(carrier = automatic)]
                    struct Bearer;
                ),
                "takes only `manual`",
            ),
            case(
                "one flow declared twice",
                quote::quote!(
                    #[security(oauth2(
                        client_credentials(token_url = "https://auth.example.com/a"),
                        client_credentials(token_url = "https://auth.example.com/b"),
                    ))]
                    struct Delegated;
                ),
                "already declared",
            ),
        ]
    }

    /// The two diagnostics that fire only where the document model has no field
    /// to hold the answer.
    ///
    /// Under `openapi32` both constructs are legal, so neither can be provoked
    /// and neither has a row. The count below adds them back, which is what
    /// keeps the ledger honest in both builds rather than in the one that
    /// happens to run first.
    #[cfg(not(feature = "openapi32"))]
    fn version_gated_ledger() -> Vec<Case> {
        vec![
            case(
                "a device authorization flow, which only 3.2 defines",
                quote::quote!(
                    #[security(oauth2(device_authorization(
                        device_authorization_url = "https://auth.example.com/device",
                        token_url = "https://auth.example.com/token"
                    )))]
                    struct Delegated;
                ),
                "openapi32",
            ),
            case(
                "an authorization server metadata URL, which only 3.2 carries",
                quote::quote!(
                    #[security(oauth2(
                        client_credentials(token_url = "https://auth.example.com/token"),
                        metadata_url = "https://auth.example.com/meta",
                    ))]
                    struct Delegated;
                ),
                "openapi32",
            ),
            case(
                "a deprecation, which only 3.2 has a field for",
                quote::quote!(
                    #[security(http(scheme = "bearer"), deprecated)]
                    struct Legacy;
                ),
                "openapi32",
            ),
        ]
    }

    #[cfg(feature = "openapi32")]
    fn version_gated_ledger() -> Vec<Case> {
        Vec::new()
    }

    /// How many diagnostics this build cannot provoke.
    ///
    /// Three, under `openapi32`: the constructs they refuse are legal there.
    const UNREACHABLE_HERE: usize = if cfg!(feature = "openapi32") { 3 } else { 0 };

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(oauth2_ledger(), expand_inner);
        each_case_is_refused(version_gated_ledger(), expand_inner);
    }

    #[test]
    fn every_security_scheme_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "security_scheme.rs",
            include_str!("security_scheme.rs"),
            ledger().len()
                + oauth2_ledger().len()
                + version_gated_ledger().len()
                + UNREACHABLE_HERE,
        );
    }

    /// A declared flow reaches the expansion.
    ///
    /// The defect this closes: `of_kind` built `OAuthFlows::default()`
    /// unconditionally and `check_kind` sent every flow to `skip_value`, so
    /// `#[security(oauth2(authorization_code(..)))]` described a scheme with no
    /// flows at all — and `examples/security_schemes.rs` shipped exactly that,
    /// emitting `{"type":"oauth2","flows":{}}` while presenting itself as the
    /// demonstration of delegated authorization.
    ///
    /// `kynos-macros` cannot depend on `kynos`, so the assertion is on the
    /// tokens rather than on the description they build;
    /// `crates/kynos/tests/derives.rs` is where the expansion is compiled.
    #[test]
    fn a_declared_flow_reaches_the_expansion() {
        let input: syn::DeriveInput = syn::parse_quote!(
            #[security(oauth2(authorization_code(
                authorization_url = "https://auth.example.com/authorize",
                token_url = "https://auth.example.com/token",
                refresh_url = "https://auth.example.com/token",
                scopes("users:read", "users:write"),
            )))]
            struct Delegated;
        );

        let expanded = expand_inner(&input)
            .expect("a well-formed oauth2 scheme")
            .to_string();

        for expected in [
            "with_authorization_code",
            "https://auth.example.com/authorize",
            "https://auth.example.com/token",
            "users:read",
            "users:write",
        ] {
            assert!(
                expanded.contains(expected),
                "the expansion never mentions {expected:?}: {expanded}"
            );
        }
    }
}

mod provider {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::provider::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "two fields of one type, which a handler could not tell apart",
                quote::quote!(
                    struct App {
                        primary: Pool,
                        replica: Pool,
                    }
                ),
                "are both",
            ),
            // Two fields, because a lone type-parameter field has no sibling
            // implementation to overlap and is left to coherence.
            case(
                "a field typed by one of the context's own type parameters",
                quote::quote!(
                    struct App<T> {
                        pool: Pool,
                        value: T,
                    }
                ),
                "own type parameters",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_provider_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("provider.rs", include_str!("provider.rs"), ledger().len());
    }
}

mod headers {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::headers::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![case(
            "a header the framework already negotiates",
            quote::quote!(
                struct Negotiation {
                    accept: String,
                }
            ),
            "must not be declared as a header parameter",
        )]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_headers_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("headers.rs", include_str!("headers.rs"), ledger().len());
    }
}

mod tag {
    use super::{Case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::tag::expand_inner;

    /// The 3.2-only members, which a 3.1 build refuses.
    ///
    /// Empty under `openapi32`, where all three are legal — the same shape
    /// [`super::security_scheme`] uses, and for the same reason: a diagnostic
    /// that only one build can provoke still has to be counted in both.
    #[cfg(not(feature = "openapi32"))]
    fn version_gated_ledger() -> Vec<Case> {
        use super::case;

        vec![
            case(
                "a summary, which only 3.2 gives a tag",
                quote::quote!(
                    #[tag(summary = "Everything about orders")]
                    struct Orders;
                ),
                "openapi32",
            ),
            case(
                "a kind, which only 3.2 gives a tag",
                quote::quote!(
                    #[tag(kind = "nav")]
                    struct Orders;
                ),
                "openapi32",
            ),
            case(
                "a parent, which only 3.2 gives a tag",
                quote::quote!(
                    #[tag(parent = Catalogue)]
                    struct Orders;
                ),
                "openapi32",
            ),
        ]
    }

    #[cfg(feature = "openapi32")]
    fn version_gated_ledger() -> Vec<Case> {
        Vec::new()
    }

    /// How many diagnostics this build cannot provoke.
    ///
    /// One, under `openapi32`: the three members share a single site, and what
    /// it refuses is legal there.
    const UNREACHABLE_HERE: usize = if cfg!(feature = "openapi32") { 1 } else { 0 };

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(version_gated_ledger(), expand_inner);
    }

    #[test]
    fn every_tag_diagnostic_has_a_case() {
        // The three members are refused from one `syn::Error::new`, so the
        // ledger's three cases meet one site. Counting the *site* is the point:
        // a fourth 3.2 member added without a case fails here.
        let covered = usize::from(!version_gated_ledger().is_empty()) + UNREACHABLE_HERE;
        every_diagnostic_has_a_case("tag.rs", include_str!("tag.rs"), covered);
    }
}
