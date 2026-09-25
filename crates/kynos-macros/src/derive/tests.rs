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
            case(
                "`#[serde(transparent)]` written through one field and read through another",
                quote::quote!(
                    #[serde(transparent)]
                    struct Split {
                        #[serde(skip_deserializing)]
                        a: u64,
                        #[serde(skip_serializing)]
                        b: String,
                    }
                ),
                "`#[serde(transparent)]` makes serde write through",
            ),
            case(
                "`skip_serializing` alone on a tuple member, which serde writes one way only",
                quote::quote!(
                    struct Pair(u64, #[serde(skip_serializing)] u64);
                ),
                "leaves this member out in one direction only",
            ),
            case(
                "`skip_serializing_if` on a tuple member that is not the last one",
                quote::quote!(
                    struct Reading(
                        #[serde(default, skip_serializing_if = "is_zero")] u64,
                        #[serde(default)] String,
                    );
                ),
                "`skip_serializing_if` on a tuple member is refused unless",
            ),
        ]
    }

    /// The refusals that depend on an enum's variants: how they are tagged, and
    /// the variant skips no one schema is true of in both directions.
    ///
    /// A third function for the reason `serde_ledger` gives: that list is at
    /// the length Clippy accepts.
    fn variant_ledger() -> Vec<Case> {
        vec![
            case(
                "an untagged variant, which serde writes as its bare payload",
                quote::quote!(
                    enum Reading {
                        Labelled {
                            value: u64,
                        },
                        #[serde(untagged)]
                        Bare(u64),
                    }
                ),
                "an untagged variant",
            ),
            case(
                "a skipped non-`Option` member of an adjacently tagged newtype variant",
                quote::quote!(
                    #[serde(tag = "t", content = "c")]
                    enum Reading {
                        Count(u64),
                        Hidden(#[serde(skip)] u64),
                    }
                ),
                "writes the variant as its tag alone",
            ),
            case(
                "`skip_deserializing` alone on a variant, which serde writes and never reads",
                quote::quote!(
                    enum Channel {
                        Web,
                        #[serde(skip_deserializing)]
                        Fax,
                    }
                ),
                "makes serde write this variant and refuse to read it back",
            ),
            case(
                "a variant's own name an earlier variant's `alias` claims",
                quote::quote!(
                    #[serde(tag = "kind")]
                    enum Signal {
                        #[serde(alias = "Stop")]
                        Start,
                        Stop,
                    }
                ),
                "this variant's own name",
            ),
        ]
    }

    /// The named-field skips no one schema is true of in both directions.
    ///
    /// A fourth function, so that a row about a field does not sit in a ledger
    /// named for variants.
    fn field_ledger() -> Vec<Case> {
        vec![
            case(
                "`skip_deserializing` alone on a field of an object `deny_unknown_fields` closes",
                quote::quote!(
                    #[serde(deny_unknown_fields)]
                    struct Thing {
                        id: u64,
                        #[serde(skip_deserializing)]
                        stamp: u64,
                    }
                ),
                "refuses a member the schema does not name",
            ),
            case(
                "an open flattened map in an object serde reads under `deny_unknown_fields`",
                quote::quote!(
                    #[serde(deny_unknown_fields)]
                    struct Thing {
                        id: u64,
                        #[serde(flatten)]
                        #[schema(open)]
                        extra: BTreeMap<String, String>,
                    }
                ),
                "reads the map empty",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(serde_ledger(), expand_inner);
        each_case_is_refused(variant_ledger(), expand_inner);
        each_case_is_refused(field_ledger(), expand_inner);
    }

    #[test]
    fn every_schema_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "schema.rs",
            include_str!("schema.rs"),
            ledger().len() + serde_ledger().len() + variant_ledger().len() + field_ledger().len(),
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

    /// An untagged variant serde skips both ways is in no schema, so it has no
    /// decoding rule to be ambiguous about and is accepted, as a skipped
    /// `#[serde(other)]` variant is.
    #[test]
    fn untagged_on_a_variant_skipped_both_ways_is_accepted() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            enum Reading {
                Labelled {
                    value: u64,
                },
                #[serde(skip, untagged)]
                Bare(u64),
            }
        ))
        .expect("the case itself must parse");

        if let Err(error) = expand_inner(&input) {
            panic!("a variant in no schema was refused: {error}");
        }
    }

    /// A name only an earlier variant's `alias` shares is dropped from the
    /// later one rather than refused, and one a variant serde skips both ways
    /// claims is claimed by nothing serde reads.
    #[test]
    fn a_shared_name_serde_still_reads_back_is_accepted() {
        for declaration in [
            quote::quote!(
                enum Signal {
                    Start,
                    #[serde(alias = "Start")]
                    Stop,
                }
            ),
            quote::quote!(
                enum Signal {
                    #[serde(alias = "go")]
                    Start,
                    #[serde(alias = "go")]
                    Stop,
                }
            ),
            quote::quote!(
                enum Signal {
                    #[serde(skip, alias = "Stop")]
                    Start,
                    Stop,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");
            if let Err(error) = expand_inner(&input) {
                panic!("a name serde reads back was refused: {error}");
            }
        }
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
                // A member serde skips in one direction only is still scanned,
                // and a newtype's member is written whatever it skips, so each
                // override here is refused before any skip rule is read.
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
    /// never carries. A skipped named field, every field of a skipped variant,
    /// and a member of a tuple, tuple variant or newtype variant that serde
    /// skips both ways are in no schema at all, so there is nothing for the
    /// override to contradict. Nor is a transparent struct's member in a
    /// direction serde picks no single member for: serde refuses that
    /// direction's derive, so no function is called there.
    #[test]
    fn a_wire_form_override_on_an_undescribed_field_is_left_alone() {
        for declaration in [
            quote::quote!(
                struct Pair(u64, #[serde(skip, with = "as_string")] u64);
            ),
            quote::quote!(
                enum Reading {
                    Count(u64, #[serde(skip, with = "as_string")] u64),
                }
            ),
            quote::quote!(
                enum Reading {
                    Total(u64),
                    Count(#[serde(skip, with = "as_string")] u64),
                }
            ),
            // A transparent struct is its one described member, and the other
            // is skipped both ways on a two-member tuple.
            quote::quote!(
                #[serde(transparent)]
                struct Pair(u64, #[serde(skip, with = "as_string")] u64);
            ),
            // serde neither writes nor reads through a transparent tuple's
            // unpicked member, whatever it skips, as with its named twin.
            quote::quote!(
                #[serde(transparent)]
                struct Pair(
                    u64,
                    #[serde(default, skip_serializing, with = "as_string")] u64,
                );
            ),
            // serde writes through member 0 alone and reads through no single
            // member, so it derives only `Serialize`: member 1 reaches neither
            // direction, and member 0 is written but never read.
            quote::quote!(
                #[serde(transparent)]
                struct Handle(
                    u64,
                    #[serde(skip_serializing, deserialize_with = "from_string")] u64,
                    #[serde(skip_serializing)] u64,
                );
            ),
            quote::quote!(
                #[serde(transparent)]
                struct Handle(
                    #[serde(deserialize_with = "from_string")] u64,
                    #[serde(skip_serializing)] u64,
                );
            ),
            // The mirror: serde reads through member 0 alone and writes through
            // no single member, so it derives only `Deserialize`.
            quote::quote!(
                #[serde(transparent)]
                struct Handle(
                    u64,
                    #[serde(default, serialize_with = "as_string")] u64,
                    #[serde(default)] u64,
                );
            ),
            quote::quote!(
                #[serde(transparent)]
                struct Handle(
                    #[serde(serialize_with = "as_string")] u64,
                    #[serde(default)] u64,
                );
            ),
            // Neither direction picks a single member, so serde derives neither
            // and refuses the struct in its own words.
            quote::quote!(
                #[serde(transparent)]
                struct Handle(#[serde(with = "as_string")] u64, u64);
            ),
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

    /// A wire-form override on a newtype struct's member is refused whatever
    /// serde skips.
    ///
    /// serde ignores skip attributes on a newtype struct and still writes its
    /// member through the function, so the exemption a skipped tuple member
    /// gets does not reach it.
    #[test]
    fn a_wire_form_override_on_a_newtype_member_is_refused_whatever_it_skips() {
        each_case_is_refused(
            vec![case(
                "`serialize_with` on the member of a newtype that skips it both ways",
                quote::quote!(
                    struct Sku(#[serde(skip, serialize_with = "as_string")] u64);
                ),
                "`serialize_with` reads or writes this field",
            )],
            expand_inner,
        );
    }

    /// A wire-form override on the member a transparent struct is written or
    /// read through is refused, for the keys of each direction that picks it.
    ///
    /// A member picked both ways is held to each one-direction key, which only
    /// the scan for all three refuses together.
    #[test]
    fn a_wire_form_override_on_a_transparent_struct_s_picked_member_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`serialize_with` on the member picked both ways",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Handle(#[serde(serialize_with = "as_string")] u64);
                    ),
                    "`serialize_with` reads or writes this field",
                ),
                case(
                    "`deserialize_with` on the member picked both ways",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Handle(#[serde(deserialize_with = "from_string")] u64);
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
                case(
                    "`serialize_with` on the one write candidate, beside a second read candidate",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Handle(
                            #[serde(serialize_with = "as_string")] u64,
                            #[serde(skip_serializing)] u64,
                        );
                    ),
                    "`serialize_with` reads or writes this field",
                ),
                case(
                    "`deserialize_with` on the one read candidate, beside a second write candidate",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Handle(
                            #[serde(deserialize_with = "from_string")] u64,
                            #[serde(default)] u64,
                        );
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
            ],
            expand_inner,
        );
    }

    /// A wire-form override on a flattened `PhantomData` serde reads is refused.
    ///
    /// The marker alone puts nothing in the object, but serde hands the
    /// function its flattening serializer, which writes whatever members the
    /// function emits, and its flattening deserializer, which reads whatever
    /// members the function demands.
    #[test]
    fn a_wire_form_override_on_a_flattened_phantom_data_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`with` on a flattened `PhantomData`",
                    quote::quote!(
                        struct Reading {
                            total: u64,
                            #[serde(flatten, with = "extra")]
                            marker: PhantomData<()>,
                        }
                    ),
                    "`with` reads or writes this field",
                ),
                case(
                    "`serialize_with` on a flattened `PhantomData`",
                    quote::quote!(
                        struct Reading {
                            total: u64,
                            #[serde(flatten, serialize_with = "extra")]
                            marker: PhantomData<()>,
                        }
                    ),
                    "`serialize_with` reads or writes this field",
                ),
                case(
                    "`deserialize_with` on a flattened `PhantomData`",
                    quote::quote!(
                        struct Reading {
                            total: u64,
                            #[serde(flatten, deserialize_with = "demand")]
                            marker: PhantomData<()>,
                        }
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
                case(
                    "`deserialize_with` on a flattened `PhantomData` serde never writes",
                    quote::quote!(
                        struct Reading {
                            total: u64,
                            #[serde(flatten, skip_serializing, deserialize_with = "demand")]
                            marker: PhantomData<()>,
                        }
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
                case(
                    "`with` on a flattened `PhantomData` in a struct variant",
                    quote::quote!(
                        enum Reading {
                            Count {
                                total: u64,
                                #[serde(flatten, with = "extra")]
                                marker: PhantomData<()>,
                            },
                        }
                    ),
                    "`with` reads or writes this field",
                ),
            ],
            expand_inner,
        );
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
                // The untagged-enum refusal would also fire here. The
                // conversion is reported instead, because every other rule
                // reads a declaration the conversion says the wire does not
                // follow.
                case(
                    "`from` on an untagged enum, which the untagged refusal also refuses",
                    quote::quote!(
                        #[serde(untagged, from = "String")]
                        enum Payload {
                            Number(u32),
                            Text(String),
                        }
                    ),
                    "`from` makes serde read or write this enum as the type it names",
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

    /// A transparent struct serde writes through one field and reads through
    /// another is refused.
    ///
    /// `serde_derive`'s `allow_transparent` writes through the field without
    /// `skip_serializing` and reads through the field without
    /// `skip_deserializing` or a field-level `default`, never a `PhantomData`.
    /// Each row is a declaration serde accepts under `Serialize`, `Deserialize`
    /// and both, and names the field each direction picks.
    #[test]
    fn a_transparent_struct_serde_writes_and_reads_apart_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "written through `a`, read through `b`",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Hole {
                            #[serde(default)]
                            a: u64,
                            #[serde(skip_serializing)]
                            b: String,
                        }
                    ),
                    "this struct writes through `a` and reads through `b`",
                ),
                case(
                    // A `default` member may follow one without it.
                    "a tuple struct written through one member and read through another",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Pair(#[serde(skip_serializing)] u64, #[serde(default)] u64);
                    ),
                    "this struct writes through field 1 and reads through field 0",
                ),
            ],
            expand_inner,
        );
    }

    /// A transparent struct serde picks a single field for is described by that
    /// field.
    ///
    /// Where both directions pick one field it is the same one. Where only one
    /// direction does, serde refuses the other derive by itself, so the struct
    /// compiles with that direction's derive alone and its one field is all
    /// serde writes, or reads. Each row names the type the schema must resolve
    /// and the type of the field it must not.
    #[test]
    fn a_transparent_struct_serde_picks_one_field_for_is_described_by_it() {
        for (declaration, described, other) in [
            // A default on a field neither direction picks changes nothing.
            (
                quote::quote!(
                    #[serde(transparent)]
                    struct Labels {
                        inner: u64,
                        #[serde(default, skip)]
                        extra: String,
                    }
                ),
                "u64",
                "String",
            ),
            // Both skips spelled apart are `skip`.
            (
                quote::quote!(
                    #[serde(transparent)]
                    struct Labels {
                        #[serde(skip_serializing, skip_deserializing)]
                        extra: String,
                        inner: u64,
                    }
                ),
                "u64",
                "String",
            ),
            (
                quote::quote!(
                    #[serde(transparent)]
                    struct Handle(u64, #[serde(skip)] String);
                ),
                "u64",
                "String",
            ),
            // Written through `a`, read through no field: serde refuses
            // `Deserialize` and accepts `Serialize` alone.
            (
                quote::quote!(
                    #[serde(transparent)]
                    struct Hole {
                        #[serde(default)]
                        a: u64,
                        #[serde(skip)]
                        b: String,
                    }
                ),
                "u64",
                "String",
            ),
            // Written through no field, read through `a`: serde refuses
            // `Serialize` and accepts `Deserialize` alone.
            (
                quote::quote!(
                    #[serde(transparent)]
                    struct Hole {
                        #[serde(skip_serializing)]
                        a: u64,
                        #[serde(skip_serializing, skip_deserializing)]
                        b: String,
                    }
                ),
                "u64",
                "String",
            ),
            // Written through `a`, read through both: serde refuses
            // `Deserialize` and accepts `Serialize` alone.
            (
                quote::quote!(
                    #[serde(transparent)]
                    struct Hole {
                        a: u64,
                        #[serde(skip_serializing)]
                        b: String,
                    }
                ),
                "u64",
                "String",
            ),
            // Written through both, read through `b`: serde refuses
            // `Serialize` and accepts `Deserialize` alone.
            (
                quote::quote!(
                    #[serde(transparent)]
                    struct Hole {
                        #[serde(skip_deserializing)]
                        a: u64,
                        b: String,
                    }
                ),
                "String",
                "u64",
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            let expanded = match expand_inner(&input) {
                Ok(tokens) => tokens.to_string(),
                Err(error) => {
                    panic!("a transparent struct with one picked field must expand: {error}")
                }
            };
            assert!(
                expanded.contains(&format!("resolve :: < {described} >"))
                    && !expanded.contains(&format!("resolve :: < {other} >")),
                "the schema must describe the `{described}` field alone: {expanded}"
            );
        }
    }

    /// A `PhantomData` a macro passed through a `$t:ty` fragment is still a
    /// `PhantomData`.
    ///
    /// rustc hands such a type to the derive inside an invisible group, which
    /// `syn` parses as `Type::Group`, and serde's transparent check unwraps it.
    /// Unrecognised, the marker would count as a member serde writes and reads,
    /// and the derive would describe it and demand `PhantomData<T>: Schema`.
    #[test]
    fn a_phantom_member_a_macro_wraps_in_a_group_is_not_described() {
        let marker =
            proc_macro2::Group::new(proc_macro2::Delimiter::None, quote::quote!(PhantomData<T>));
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(transparent)]
            struct Id<T>(u64, #marker);
        ))
        .expect("the case itself must parse");

        // Without the group there is nothing here to test.
        let syn::Data::Struct(data) = &input.data else {
            panic!("the case is a struct");
        };
        assert!(
            data.fields
                .iter()
                .any(|field| matches!(field.ty, syn::Type::Group(_))),
            "the marker did not parse as a `Type::Group`"
        );

        let expanded = match expand_inner(&input) {
            Ok(tokens) => tokens.to_string(),
            Err(error) => panic!("a transparent struct beside a marker must expand: {error}"),
        };
        assert!(
            !expanded.contains("PhantomData"),
            "the marker reached the expansion: {expanded}"
        );
    }

    /// A transparent struct serde refuses in both directions is serde's to
    /// refuse.
    ///
    /// With no single field to write through and none to read through, serde
    /// raises its own error for either derive, so a second one here would
    /// restate a serde shape rule, for the reason
    /// `untagged_on_a_struct_is_left_to_serde` gives.
    #[test]
    fn a_transparent_struct_serde_refuses_both_ways_is_left_to_serde() {
        for declaration in [
            quote::quote!(
                #[serde(transparent)]
                struct Two {
                    a: u64,
                    b: String,
                }
            ),
            quote::quote!(
                #[serde(transparent)]
                struct Empty {
                    #[serde(skip)]
                    a: u64,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            let Err(error) = expand_inner(&input) else {
                continue;
            };

            assert!(
                !error.to_string().contains("`#[serde(transparent)]`"),
                "a struct serde refuses both ways drew a second refusal: {error}"
            );
        }
    }

    /// A catch-all serde never writes is refused all the same.
    ///
    /// `#[serde(other)]` is not about the variant's own branch:
    /// `skip_serializing` keeps the variant out of what serde writes, but
    /// deserialization still routes every tag the enum does not name to it, so
    /// the schema's closed `oneOf` still disagrees with what the type accepts.
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

    /// A catch-all on a variant serde never reads is left alone.
    ///
    /// serde draws the fallthrough only from the variants it reads, so
    /// `#[serde(other)]` on one it skips on read catches nothing, and the
    /// refusal's "accepts every tag this enum does not name" would be false. A
    /// lone `skip_deserializing` never reaches the check, since
    /// `reject_unread_variant` refuses it first.
    #[test]
    fn a_catch_all_on_a_variant_serde_never_reads_is_left_alone() {
        for declaration in [
            quote::quote!(
                #[serde(tag = "kind")]
                enum Event {
                    Created {
                        id: u64,
                    },
                    #[serde(skip)]
                    #[serde(other)]
                    Unknown,
                }
            ),
            quote::quote!(
                #[serde(tag = "kind")]
                enum Event {
                    Created {
                        id: u64,
                    },
                    #[serde(skip_serializing)]
                    #[serde(skip_deserializing)]
                    #[serde(other)]
                    Unknown,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            if let Err(error) = expand_inner(&input) {
                panic!("a catch-all serde never reads must expand: {error}");
            }
        }
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
            // A transparent struct is its one field's value, which serde
            // writes whatever `skip_serializing_if` says, so there is no
            // `required` list for the field to contradict.
            quote::quote!(
                #[serde(transparent)]
                struct Draft {
                    #[serde(skip_serializing_if = "String::is_empty")]
                    elided: String,
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

    /// A member serde leaves out in one direction only is refused wherever it
    /// holds a position or a variant's payload, under each key and tagging.
    ///
    /// One row per key and placement: the ledger's single row proves the site
    /// fires, and this proves the walk reaches every placement and names the
    /// key that was written.
    #[test]
    fn every_one_way_member_skip_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`skip_serializing` on a tuple member",
                    quote::quote!(
                        struct Pair(u64, #[serde(skip_serializing)] u64);
                    ),
                    "`skip_serializing` leaves this member out in one direction only",
                ),
                case(
                    "`skip_deserializing` on a tuple member",
                    quote::quote!(
                        struct Pair(#[serde(skip_deserializing)] u64, String);
                    ),
                    "`skip_deserializing` leaves this member out in one direction only",
                ),
                case(
                    "`skip_serializing` on a tuple-variant member",
                    quote::quote!(
                        enum Reading {
                            Count(u64, #[serde(skip_serializing)] u64),
                        }
                    ),
                    "`skip_serializing` leaves this member out in one direction only",
                ),
                case(
                    "`skip_deserializing` on a tuple-variant member",
                    quote::quote!(
                        enum Reading {
                            Count(#[serde(skip_deserializing)] u64, u64),
                        }
                    ),
                    "`skip_deserializing` leaves this member out in one direction only",
                ),
                case(
                    "`skip_serializing` on an externally tagged newtype-variant member",
                    quote::quote!(
                        enum Reading {
                            Count(#[serde(skip_serializing)] u64),
                        }
                    ),
                    "`skip_serializing` leaves this member out in one direction only",
                ),
                case(
                    "`skip_deserializing` on an adjacently tagged newtype-variant member",
                    quote::quote!(
                        #[serde(tag = "t", content = "c")]
                        enum Reading {
                            Count(#[serde(skip_deserializing)] u64),
                        }
                    ),
                    "`skip_deserializing` leaves this member out in one direction only",
                ),
                case(
                    "`skip_serializing` on an internally tagged newtype-variant member",
                    quote::quote!(
                        #[serde(tag = "t")]
                        enum Reading {
                            Count(#[serde(skip_serializing)] Audit),
                        }
                    ),
                    "`skip_serializing` leaves this member out in one direction only",
                ),
            ],
            expand_inner,
        );
    }

    /// `skip_serializing_if` on a tuple member is refused unless it is the last
    /// position and carries `#[serde(default)]`, on a tuple struct and a tuple
    /// variant alike.
    #[test]
    fn every_misplaced_skip_serializing_if_on_a_tuple_member_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`skip_serializing_if` on a member a later position follows",
                    quote::quote!(
                        struct Reading(
                            #[serde(default, skip_serializing_if = "is_zero")] u64,
                            #[serde(default)] String,
                        );
                    ),
                    "`skip_serializing_if` on a tuple member is refused unless",
                ),
                case(
                    "`skip_serializing_if` on the last member, without a default",
                    quote::quote!(
                        struct Reading(u64, #[serde(skip_serializing_if = "is_zero")] u64);
                    ),
                    "`skip_serializing_if` on a tuple member is refused unless",
                ),
                case(
                    "`skip_serializing_if` on a tuple-variant member a later position follows",
                    quote::quote!(
                        enum Reading {
                            Count(
                                #[serde(default, skip_serializing_if = "is_zero")] u64,
                                #[serde(default)] u64,
                            ),
                        }
                    ),
                    "`skip_serializing_if` on a tuple member is refused unless",
                ),
            ],
            expand_inner,
        );
    }

    /// A skipped member of an adjacently tagged newtype variant is refused
    /// unless its type is an `Option`, however the skip is spelt.
    ///
    /// serde writes such a variant as the tag alone and reads it only beside
    /// its content, which an `Option` member alone may leave out. A variant
    /// serde reads and never writes is described by the same tag-only branch,
    /// which serde still refuses to read, so it is refused as well.
    #[test]
    fn every_skipped_adjacently_tagged_payload_is_refused_unless_optional() {
        each_case_is_refused(
            vec![
                case(
                    "`skip` on an adjacently tagged newtype-variant member",
                    quote::quote!(
                        #[serde(tag = "t", content = "c")]
                        enum Reading {
                            Hidden(#[serde(skip)] u64),
                        }
                    ),
                    "`skip` leaves out the only member",
                ),
                case(
                    "`skip_serializing` beside `skip_deserializing` on that member",
                    quote::quote!(
                        #[serde(tag = "t", content = "c")]
                        enum Reading {
                            Hidden(#[serde(skip_serializing, skip_deserializing)] String),
                        }
                    ),
                    "`skip_serializing` leaves out the only member",
                ),
                case(
                    "`skip` on a member whose path type is not an `Option`",
                    quote::quote!(
                        #[serde(tag = "t", content = "c", rename_all = "snake_case")]
                        enum Reading {
                            Count(u64),
                            Hidden(#[serde(skip)] std::vec::Vec<u64>),
                        }
                    ),
                    "`skip` leaves out the only member",
                ),
                case(
                    "`skip` on the member of a variant serde reads and never writes",
                    quote::quote!(
                        #[serde(tag = "t", content = "c")]
                        enum Reading {
                            Count(u64),
                            #[serde(skip_serializing)]
                            Hidden(#[serde(skip)] u64),
                        }
                    ),
                    "`skip` leaves out the only member",
                ),
            ],
            expand_inner,
        );
    }

    /// Every member skip serde honours in both directions expands.
    ///
    /// Each row is a placement the refusal must not reach: a newtype struct,
    /// whose skips serde ignores; a member skipped both ways, however it is
    /// spelt; a trailing `skip_serializing_if` beside its default, last among
    /// the positions even when a skipped member follows it; a newtype
    /// variant's `skip_serializing_if`, which serde ignores; a skipped
    /// variant; and a transparent struct, which holds no positions.
    #[test]
    fn a_member_skip_serde_honours_both_ways_is_accepted() {
        for declaration in [
            quote::quote!(
                struct Sku(#[serde(skip_serializing)] u64);
            ),
            quote::quote!(
                struct Sku(#[serde(skip_deserializing)] u64);
            ),
            quote::quote!(
                struct Sku(#[serde(skip_serializing_if = "is_zero")] u64);
            ),
            quote::quote!(
                struct Pair(#[serde(skip)] u64, String);
            ),
            quote::quote!(
                struct Pair(
                    u64,
                    #[serde(skip_serializing)]
                    #[serde(skip_deserializing)]
                    u64,
                );
            ),
            quote::quote!(
                struct Tally(u64, #[serde(default, skip_serializing_if = "is_zero")] u64);
            ),
            quote::quote!(
                struct Tally(
                    u64,
                    #[serde(default, skip_serializing_if = "is_zero")] u64,
                    #[serde(skip)] u64,
                );
            ),
            quote::quote!(
                enum Reading {
                    Count(#[serde(skip_serializing_if = "is_zero")] u64),
                }
            ),
            quote::quote!(
                #[serde(tag = "t")]
                enum Reading {
                    Total(Audit),
                    #[serde(skip)]
                    Count(#[serde(skip_serializing)] u64, u64),
                }
            ),
            // `default` keeps serde from reading through the second member, so
            // the transparent refusal accepts it and only this one could fire.
            quote::quote!(
                #[serde(transparent)]
                struct Handle(u64, #[serde(default, skip_serializing)] u64);
            ),
            // A container `default` fills every missing trailing element.
            quote::quote!(
                #[serde(default)]
                struct Tally(u64, #[serde(skip_serializing_if = "is_zero")] u64);
            ),
            // A skipped newtype variant serde reads back as it writes: an
            // `Option` member under adjacent tagging, and any member under
            // external or internal tagging.
            quote::quote!(
                #[serde(tag = "t", content = "c")]
                enum Reading {
                    Hidden(#[serde(skip)] Option<u64>),
                }
            ),
            quote::quote!(
                enum Reading {
                    Hidden(#[serde(skip)] u64),
                }
            ),
            quote::quote!(
                #[serde(tag = "t")]
                enum Reading {
                    Total(Audit),
                    Hidden(#[serde(skip)] u64),
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            if let Err(error) = expand_inner(&input) {
                panic!("a member skip serde honours both ways must expand: {error}");
            }
        }
    }

    /// A variant skip serde reads through expands, and so does whatever serde
    /// never reaches inside a variant it never writes.
    ///
    /// serde's `Serialize` arm for a `skip_serializing` variant errors before it
    /// touches a field, so a field's `serialize_with`, any `skip_serializing_if`
    /// and a member's lone `skip_serializing` change nothing serde does with the
    /// variant, and refusing one would be a false refusal. The two-attribute
    /// spelling of a skip both ways is here so it is not read as either half.
    #[test]
    fn a_variant_skip_serde_reads_through_is_accepted() {
        for declaration in [
            quote::quote!(
                enum Channel {
                    Web,
                    #[serde(skip_serializing)]
                    Fax,
                }
            ),
            quote::quote!(
                enum Channel {
                    Web,
                    #[serde(skip_serializing)]
                    #[serde(skip_deserializing)]
                    Fax,
                }
            ),
            quote::quote!(
                enum Reading {
                    Total(u64),
                    #[serde(skip_serializing)]
                    Count {
                        #[serde(serialize_with = "as_string")]
                        count: u64,
                    },
                }
            ),
            quote::quote!(
                #[serde(tag = "kind")]
                enum Event {
                    Created {
                        at: String,
                    },
                    #[serde(skip_serializing)]
                    Amended {
                        #[serde(skip_serializing_if = "String::is_empty")]
                        note: String,
                    },
                }
            ),
            quote::quote!(
                enum Reading {
                    Total(u64),
                    #[serde(skip_serializing)]
                    Count(u64, #[serde(skip_serializing)] u64),
                }
            ),
            quote::quote!(
                enum Reading {
                    Total(u64),
                    #[serde(skip_serializing)]
                    Count(#[serde(skip_serializing_if = "is_zero")] u64, u64),
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            if let Err(error) = expand_inner(&input) {
                panic!("a variant skip serde reads through must expand: {error}");
            }
        }
    }

    /// Inside a variant serde reads and never writes, what serde reads through
    /// is refused as it is in a variant serde writes.
    #[test]
    fn a_read_override_or_skip_inside_a_variant_serde_never_writes_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`deserialize_with` on a field of a variant serde never writes",
                    quote::quote!(
                        enum Reading {
                            Total(u64),
                            #[serde(skip_serializing)]
                            Count {
                                #[serde(deserialize_with = "from_string")]
                                count: u64,
                            },
                        }
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
                case(
                    "`with` on a field of a variant serde never writes",
                    quote::quote!(
                        enum Reading {
                            Total(u64),
                            #[serde(skip_serializing)]
                            Count {
                                #[serde(with = "as_string")]
                                count: u64,
                            },
                        }
                    ),
                    "`with` reads or writes this field",
                ),
                case(
                    "`deserialize_with` on a variant serde never writes",
                    quote::quote!(
                        enum Reading {
                            Total(u64),
                            #[serde(skip_serializing, deserialize_with = "from_string")]
                            Count(u64),
                        }
                    ),
                    "`deserialize_with` reads or writes this variant",
                ),
                case(
                    "`skip_deserializing` alone on a member of a variant serde never writes",
                    quote::quote!(
                        enum Reading {
                            Total(u64),
                            #[serde(skip_serializing)]
                            Count(u64, #[serde(skip_deserializing)] u64),
                        }
                    ),
                    "`skip_deserializing` leaves this member out in one direction only",
                ),
            ],
            expand_inner,
        );
    }

    /// A named field serde reads and never writes is refused wherever serde
    /// still requires it on read, by the rule `skip_serializing_if` meets.
    ///
    /// `skip_serializing` alone is `skip_serializing_if` with a condition that
    /// always holds: serde leaves the field out of every object it writes, and
    /// without a default refuses an object without it on read. One row per
    /// placement: a struct, a struct variant under each tagging, and a
    /// flattened struct, which no default covers.
    #[test]
    fn a_named_field_serde_requires_but_never_writes_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`skip_serializing` alone on a struct field with no default",
                    quote::quote!(
                        struct Draft {
                            plain: u64,
                            #[serde(skip_serializing)]
                            elided: u64,
                        }
                    ),
                    "`skip_serializing` lets serde leave this field out of what it writes",
                ),
                case(
                    "`skip_serializing` alone on an externally tagged variant field",
                    quote::quote!(
                        enum Event {
                            Created {
                                at: u64,
                                #[serde(skip_serializing)]
                                note: u64,
                            },
                        }
                    ),
                    "`skip_serializing` lets serde leave this field out of what it writes",
                ),
                case(
                    "`skip_serializing` alone on an internally tagged variant field",
                    quote::quote!(
                        #[serde(tag = "kind")]
                        enum Event {
                            Created {
                                at: u64,
                                #[serde(skip_serializing)]
                                note: u64,
                            },
                        }
                    ),
                    "`skip_serializing` lets serde leave this field out of what it writes",
                ),
                case(
                    "`skip_serializing` alone on an adjacently tagged variant field",
                    quote::quote!(
                        #[serde(tag = "kind", content = "body")]
                        enum Event {
                            Created {
                                at: u64,
                                #[serde(skip_serializing)]
                                note: u64,
                            },
                        }
                    ),
                    "`skip_serializing` lets serde leave this field out of what it writes",
                ),
                case(
                    "`skip_serializing` alone on a flattened struct, beside a default",
                    quote::quote!(
                        struct Wrapper {
                            id: u64,
                            #[serde(flatten, default, skip_serializing)]
                            audit: Audit,
                        }
                    ),
                    "`skip_serializing` on a flattened field is refused unless it is \
                     `#[schema(open)]`",
                ),
            ],
            expand_inner,
        );
    }

    /// On a named field serde reads and never writes, the overrides serde
    /// reads through are refused, as they are inside a variant serde never
    /// writes.
    #[test]
    fn a_read_override_on_a_named_field_serde_never_writes_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "`deserialize_with` on a field serde never writes",
                    quote::quote!(
                        struct Reading {
                            total: u64,
                            #[serde(skip_serializing, default, deserialize_with = "from_string")]
                            count: u64,
                        }
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
                case(
                    "`with` on a field serde never writes",
                    quote::quote!(
                        struct Reading {
                            total: u64,
                            #[serde(skip_serializing, default, with = "as_string")]
                            count: u64,
                        }
                    ),
                    "`with` reads or writes this field",
                ),
            ],
            expand_inner,
        );
    }

    /// A named field serde skips in one direction expands wherever one schema is
    /// true of both.
    ///
    /// Read and never written, it is a property `required` leaves out beside an
    /// `Option` or a default, a property of a variant serde never writes, and a
    /// flattened open map; `serialize_with` on it changes nothing serde does.
    /// Written and never read, it is left out of an object that constrains no
    /// member it does not name, whatever it writes through. A transparent struct
    /// is scanned over the fields serde writes or reads through, and no other.
    #[test]
    fn a_named_field_serde_skips_in_one_direction_is_accepted() {
        for declaration in [
            quote::quote!(
                struct Draft {
                    plain: u64,
                    #[serde(skip_serializing, default)]
                    elided: u64,
                }
            ),
            quote::quote!(
                struct Draft {
                    plain: u64,
                    #[serde(skip_serializing)]
                    maybe: Option<u64>,
                }
            ),
            quote::quote!(
                #[serde(default)]
                struct Draft {
                    plain: u64,
                    #[serde(skip_serializing)]
                    elided: u64,
                }
            ),
            quote::quote!(
                enum Reading {
                    Total(u64),
                    #[serde(skip_serializing)]
                    Count {
                        #[serde(skip_serializing)]
                        count: u64,
                    },
                }
            ),
            quote::quote!(
                struct Draft {
                    id: u64,
                    #[serde(flatten, skip_serializing)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            ),
            quote::quote!(
                struct Reading {
                    total: u64,
                    #[serde(skip_serializing, default, serialize_with = "as_string")]
                    count: u64,
                }
            ),
            quote::quote!(
                struct Draft {
                    plain: u64,
                    #[serde(skip_deserializing)]
                    stamp: u64,
                }
            ),
            quote::quote!(
                struct Reading {
                    total: u64,
                    #[serde(skip_deserializing, serialize_with = "as_string")]
                    count: u64,
                }
            ),
            // A flattened `PhantomData` serde never reads is exempt like any
            // other named field serde never reads, and one serde never writes
            // is only read, where `serialize_with` changes nothing.
            quote::quote!(
                struct Reading {
                    total: u64,
                    #[serde(flatten, skip_deserializing, serialize_with = "extra")]
                    marker: PhantomData<()>,
                }
            ),
            quote::quote!(
                struct Reading {
                    total: u64,
                    #[serde(flatten, skip_serializing, serialize_with = "extra")]
                    marker: PhantomData<()>,
                }
            ),
            // serde writes and reads through `a` alone, so `b` reaches neither
            // direction and nothing it reads through can contradict the schema.
            quote::quote!(
                #[serde(transparent)]
                struct Skipped {
                    a: u64,
                    #[serde(default, skip_serializing, deserialize_with = "from_string")]
                    b: u64,
                }
            ),
            quote::quote!(
                #[serde(transparent)]
                struct Handle(
                    u64,
                    #[serde(default, skip_serializing, deserialize_with = "from_string")] u64,
                );
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            if let Err(error) = expand_inner(&input) {
                panic!("a named field one schema describes both ways must expand: {error}");
            }
        }
    }

    /// Beside an open flattened field, a field serde never writes, or never
    /// writes and never reads, expands.
    ///
    /// Skipped both ways, however it is spelt, the field is in nothing serde
    /// writes. An open map serde never reads is in no schema, so it gives the
    /// object no `unevaluatedProperties`. Inside a variant serde never writes,
    /// serde reads the field into the map, whose value schema the object
    /// applies to it.
    #[test]
    fn a_field_serde_never_writes_beside_an_open_map_is_accepted() {
        for declaration in [
            quote::quote!(
                struct Thing {
                    id: u64,
                    #[serde(skip)]
                    stamp: u64,
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            ),
            quote::quote!(
                struct Thing {
                    id: u64,
                    #[serde(skip_serializing)]
                    #[serde(skip_deserializing)]
                    stamp: u64,
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            ),
            quote::quote!(
                struct Thing {
                    id: u64,
                    #[serde(flatten, skip_deserializing)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            ),
            quote::quote!(
                enum Event {
                    Now(u64),
                    #[serde(skip_serializing)]
                    Queued {
                        at: u64,
                        #[serde(skip_deserializing)]
                        stamp: u64,
                        #[serde(flatten)]
                        #[schema(open)]
                        extra: BTreeMap<String, String>,
                    },
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            if let Err(error) = expand_inner(&input) {
                panic!("a field serde never writes beside an open map must expand: {error}");
            }
        }
    }

    /// The `AdmitsAny` and `OpenMap` witnesses the expansion asserts for the
    /// input, in that order.
    fn open_witnesses_in(declaration: TokenStream2) -> (usize, usize) {
        let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
        let expansion = match expand_inner(&input) {
            Ok(expansion) => expansion.to_string(),
            Err(error) => panic!("the case must expand: {error}"),
        };
        (
            expansion.matches("admits_any ::").count(),
            expansion.matches("is_open_map ::").count(),
        )
    }

    /// Beside a named field serde writes and never reads, an open flattened
    /// field is bounded by `AdmitsAny` rather than refused, in every object
    /// serde writes.
    ///
    /// Left out of the schema, the field is a member the object does not name,
    /// so what refuses it is the `unevaluatedProperties` the open field's type
    /// hoists, if it hoists one: `Unchecked` does not, a map does. That is the
    /// type's answer, so the rule is a bound. One row per placement, as for the
    /// closed object: a field, a flattened struct, and a struct variant under
    /// the tagging that nests it and the one that does not.
    #[test]
    fn a_field_serde_never_reads_bounds_the_open_field_beside_it_by_admits_any() {
        for declaration in [
            quote::quote!(
                struct Thing {
                    id: u64,
                    #[serde(skip_deserializing)]
                    stamp: u64,
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            ),
            quote::quote!(
                struct Thing {
                    id: u64,
                    #[serde(flatten, skip_deserializing)]
                    audit: Audit,
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            ),
            quote::quote!(
                enum Event {
                    Created {
                        at: u64,
                        #[serde(skip_deserializing)]
                        stamp: u64,
                        #[serde(flatten)]
                        #[schema(open)]
                        extra: BTreeMap<String, String>,
                    },
                }
            ),
            quote::quote!(
                #[serde(tag = "kind")]
                enum Event {
                    Created {
                        at: u64,
                        #[serde(skip_deserializing)]
                        stamp: u64,
                        #[serde(flatten)]
                        #[schema(open)]
                        extra: BTreeMap<String, String>,
                    },
                }
            ),
        ] {
            assert_eq!(open_witnesses_in(declaration), (1, 0));
        }

        // Without such a field beside it, the open field answers to `OpenMap`
        // alone, and so does one in a variant serde never writes.
        assert_eq!(
            open_witnesses_in(quote::quote!(
                struct Thing {
                    id: u64,
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            )),
            (0, 1)
        );
        assert_eq!(
            open_witnesses_in(quote::quote!(
                enum Event {
                    Now(u64),
                    #[serde(skip_serializing)]
                    Queued {
                        at: u64,
                        #[serde(skip_deserializing)]
                        stamp: u64,
                        #[serde(flatten)]
                        #[schema(open)]
                        extra: BTreeMap<String, String>,
                    },
                }
            )),
            (0, 1)
        );
    }

    /// A `#[serde(transparent)]` struct is its one field's value, with no
    /// object for an open field's `unevaluatedProperties` to reach a field
    /// serde never reads in, so the open field keeps its `OpenMap` bound alone.
    ///
    /// serde writes this one through both fields, which serde itself refuses
    /// for `Serialize`, and reads it through `extra` alone, which is the
    /// derive it accepts; `reject_transparent_without_one_field` leaves the
    /// disagreement to serde, so the struct reaches the witnesses.
    #[test]
    fn a_transparent_struct_bounds_no_open_field_by_admits_any() {
        assert_eq!(
            open_witnesses_in(quote::quote!(
                #[serde(transparent)]
                struct Thing {
                    #[serde(skip_deserializing)]
                    stamp: u64,
                    #[serde(flatten)]
                    #[schema(open)]
                    extra: BTreeMap<String, String>,
                }
            )),
            (0, 1)
        );
    }

    /// A named field serde writes and never reads is refused in every object
    /// `deny_unknown_fields` closes, since the closed object refuses what serde
    /// writes of it. One row per placement: a struct, and a struct variant
    /// under each tagging.
    #[test]
    fn a_field_serde_never_reads_in_a_closed_object_is_refused() {
        let expects = "`#[serde(deny_unknown_fields)]` gives this object";
        each_case_is_refused(
            vec![
                case(
                    "`skip_deserializing` alone on a field of a closed struct",
                    quote::quote!(
                        #[serde(deny_unknown_fields)]
                        struct Thing {
                            id: u64,
                            #[serde(skip_deserializing)]
                            stamp: u64,
                        }
                    ),
                    expects,
                ),
                case(
                    "`skip_deserializing` alone on a flattened struct in a closed struct",
                    quote::quote!(
                        #[serde(deny_unknown_fields)]
                        struct Thing {
                            id: u64,
                            #[serde(flatten, skip_deserializing)]
                            audit: Audit,
                        }
                    ),
                    expects,
                ),
                case(
                    "`skip_deserializing` alone in a closed externally tagged variant",
                    quote::quote!(
                        #[serde(deny_unknown_fields)]
                        enum Event {
                            Created {
                                at: u64,
                                #[serde(skip_deserializing)]
                                stamp: u64,
                            },
                        }
                    ),
                    expects,
                ),
                case(
                    "`skip_deserializing` alone in a closed internally tagged variant",
                    quote::quote!(
                        #[serde(deny_unknown_fields, tag = "kind")]
                        enum Event {
                            Created {
                                at: u64,
                                #[serde(skip_deserializing)]
                                stamp: u64,
                            },
                        }
                    ),
                    expects,
                ),
                case(
                    "`skip_deserializing` alone in a closed adjacently tagged variant",
                    quote::quote!(
                        #[serde(deny_unknown_fields, tag = "kind", content = "value")]
                        enum Event {
                            Created {
                                at: u64,
                                #[serde(skip_deserializing)]
                                stamp: u64,
                            },
                        }
                    ),
                    expects,
                ),
            ],
            expand_inner,
        );
    }

    /// An open map is refused wherever serde reads it into an object
    /// `deny_unknown_fields` closes, including a variant serde never writes,
    /// since the object is closed on read alone. `open` on a field that is not
    /// flattened keeps the diagnostic naming that mistake.
    #[test]
    fn an_open_map_in_a_closed_object_is_refused_in_every_group() {
        each_case_is_refused(
            vec![
                case(
                    "an open map in a closed internally tagged variant",
                    quote::quote!(
                        #[serde(deny_unknown_fields, tag = "kind")]
                        enum Event {
                            Created {
                                at: u64,
                                #[serde(flatten)]
                                #[schema(open)]
                                extra: BTreeMap<String, String>,
                            },
                        }
                    ),
                    "reads the map empty",
                ),
                case(
                    "an open map in a closed variant serde never writes",
                    quote::quote!(
                        #[serde(deny_unknown_fields)]
                        enum Event {
                            Now(u64),
                            #[serde(skip_serializing)]
                            Queued {
                                at: u64,
                                #[serde(flatten)]
                                #[schema(open)]
                                extra: BTreeMap<String, String>,
                            },
                        }
                    ),
                    "reads the map empty",
                ),
                case(
                    "`open` on a field that is not flattened, in a closed object",
                    quote::quote!(
                        #[serde(deny_unknown_fields)]
                        struct Thing {
                            #[schema(open)]
                            extra: BTreeMap<String, String>,
                        }
                    ),
                    "only a flattened field has anything",
                ),
            ],
            expand_inner,
        );
    }

    /// Under `deny_unknown_fields`, what serde reads and writes alike, or
    /// neither, expands: a field it skips both ways, a flattened struct, an
    /// `alias` on a field it never reads, and a `#[serde(transparent)]` struct,
    /// whose wire form is its one field's value rather than a closed object.
    /// So does an `alias` on a field it reads, in a struct and in a variant it
    /// never writes alike, since the closed object names every alias.
    #[test]
    fn a_closed_object_serde_agrees_with_expands() {
        for declaration in [
            quote::quote!(
                #[serde(deny_unknown_fields)]
                struct Thing {
                    #[serde(alias = "identifier")]
                    id: u64,
                }
            ),
            quote::quote!(
                #[serde(deny_unknown_fields)]
                enum Event {
                    Now(u64),
                    #[serde(skip_serializing)]
                    Queued {
                        #[serde(alias = "when")]
                        at: u64,
                    },
                }
            ),
            quote::quote!(
                #[serde(deny_unknown_fields)]
                struct Thing {
                    id: u64,
                    #[serde(skip)]
                    cache: u64,
                    #[serde(flatten)]
                    audit: Audit,
                }
            ),
            quote::quote!(
                #[serde(deny_unknown_fields)]
                struct Thing {
                    id: u64,
                    #[serde(skip, alias = "cached")]
                    cache: u64,
                }
            ),
            quote::quote!(
                #[serde(deny_unknown_fields, transparent)]
                struct Thing {
                    #[serde(alias = "identifier")]
                    id: u64,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");

            if let Err(error) = expand_inner(&input) {
                panic!("a closed object serde agrees with must expand: {error}");
            }
        }
    }

    /// A shape `deny_unknown_fields` closes does not claim to be flattenable,
    /// and one it leaves open still does.
    ///
    /// A closed object's `additionalProperties` or `unevaluatedProperties`
    /// would, inside the `allOf` one level up, refuse the members the outer
    /// object declared itself. serde leaves an internally tagged unit variant
    /// open. An externally tagged enum claims nothing either way, since its
    /// object branches are closed without the attribute.
    #[test]
    fn a_closed_shape_does_not_claim_flatten() {
        assert!(!claims_flatten(quote::quote!(
            #[serde(deny_unknown_fields)]
            struct Audit {
                at: String,
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            #[serde(deny_unknown_fields, tag = "kind")]
            enum Shape {
                Circle { radius: f64 },
                Empty,
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            #[serde(deny_unknown_fields, tag = "kind", content = "value")]
            enum Payload {
                Number(u32),
            }
        )));

        assert!(claims_flatten(quote::quote!(
            #[serde(deny_unknown_fields, tag = "kind")]
            enum Marker {
                On,
                Off,
            }
        )));
        assert!(!claims_flatten(quote::quote!(
            #[serde(deny_unknown_fields)]
            enum Command {
                Move { x: u64 },
            }
        )));
    }

    /// Whether the expansion claims `kynos::schema::flatten::Flatten` for the input.
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
            .contains(":: kynos :: schema :: flatten :: Flatten for")
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

        // Externally tagged: an object branch admits its variant key alone,
        // which inside an `allOf` one level up would refuse the members the
        // outer object declared itself; a unit variant is a bare string.
        assert!(!claims_flatten(quote::quote!(
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
            #[serde(tag = "kind")]
            enum Event {
                Created {
                    at: String,
                },
                #[serde(skip)]
                Raw(String),
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

    /// A variant serde reads and never writes has a branch, so it decides the
    /// claim to Flatten as a variant serde writes does.
    #[test]
    fn a_variant_serde_only_reads_counts_toward_the_flatten_claim() {
        assert!(!claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Shape {
                Circle {
                    radius: f64,
                },
                #[serde(skip_serializing)]
                Raw(Audit),
            }
        )));
        // Skipped both ways, the same newtype variant reaches no branch, so the
        // case isolates the direction rather than the shape.
        assert!(claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Shape {
                Circle {
                    radius: f64,
                },
                #[serde(skip_serializing)]
                #[serde(skip_deserializing)]
                Raw(Audit),
            }
        )));
    }

    /// A shape carrying a named field serde writes and never reads, where serde
    /// writes it beside the members it contributes, does not claim Flatten.
    ///
    /// The field is left out of the schema, so one level up it is a member the
    /// flattened schema does not name, and an open map beside it refuses what
    /// serde writes. A struct and an internally tagged struct variant serde
    /// writes put the field there. An adjacently tagged variant nests it under
    /// the content key, and a variant serde never writes writes nothing, so
    /// neither withdraws the claim.
    #[test]
    fn a_field_serde_never_reads_withdraws_the_flatten_claim() {
        assert!(!claims_flatten(quote::quote!(
            struct Stamped {
                id: u64,
                #[serde(skip_deserializing)]
                stamp: u64,
            }
        )));
        // Skipped both ways, or only never written, the same field leaves the
        // claim, so the case isolates the direction rather than the shape.
        assert!(claims_flatten(quote::quote!(
            struct Stamped {
                id: u64,
                #[serde(skip)]
                stamp: u64,
            }
        )));
        assert!(claims_flatten(quote::quote!(
            struct Stamped {
                id: u64,
                #[serde(skip_serializing, default)]
                stamp: u64,
            }
        )));

        assert!(!claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Event {
                Created {
                    at: u64,
                    #[serde(skip_deserializing)]
                    stamp: u64,
                },
            }
        )));
        assert!(claims_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Event {
                Created {
                    at: u64,
                },
                #[serde(skip_serializing)]
                Amended {
                    at: u64,
                    #[serde(skip_deserializing)]
                    stamp: u64,
                },
            }
        )));
        assert!(claims_flatten(quote::quote!(
            #[serde(tag = "kind", content = "data")]
            enum Event {
                Created {
                    at: u64,
                    #[serde(skip_deserializing)]
                    stamp: u64,
                },
            }
        )));
    }

    /// How many `Flatten` witnesses the expansion asserts for the input, read
    /// off the emitted tokens as [`claims_flatten`] reads its claim.
    fn flatten_witnesses_in(declaration: TokenStream2) -> usize {
        let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
        let expansion = expand_inner(&input).expect("the case itself must expand");
        expansion.to_string().matches("is_flattenable ::").count()
    }

    /// A variant serde reads and never writes composes what serde reads, so its
    /// payload and its flattened fields answer to the bound a written variant's
    /// do.
    #[test]
    fn a_variant_serde_only_reads_bounds_what_it_composes() {
        assert_eq!(
            flatten_witnesses_in(quote::quote!(
                #[serde(tag = "kind")]
                enum Shape {
                    Circle {
                        radius: f64,
                    },
                    #[serde(skip_serializing)]
                    Raw(Audit),
                }
            )),
            1
        );
        assert_eq!(
            flatten_witnesses_in(quote::quote!(
                enum Event {
                    Created {
                        at: String,
                    },
                    #[serde(skip_serializing)]
                    Amended {
                        at: String,
                        #[serde(flatten)]
                        audit: Audit,
                    },
                }
            )),
            1
        );
        assert_eq!(
            flatten_witnesses_in(quote::quote!(
                #[serde(tag = "kind")]
                enum Shape {
                    Circle {
                        radius: f64,
                    },
                    #[serde(skip)]
                    Raw(Audit),
                }
            )),
            0
        );
    }

    /// Whether the expansion claims `kynos::schema::flatten::ClosedFlatten` for the
    /// input, read off the emitted tokens as [`claims_flatten`] reads its claim.
    fn claims_closed_flatten(declaration: TokenStream2) -> bool {
        let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
        let expansion = expand_inner(&input).expect("the case itself must expand");
        expansion
            .to_string()
            .contains(":: kynos :: schema :: flatten :: ClosedFlatten for")
    }

    /// Only a flattenable shape serde reads through `deserialize_struct` claims
    /// `ClosedFlatten`.
    ///
    /// Under a closed parent serde refuses every key no flattened field took,
    /// and only `deserialize_struct` takes keys: a named struct with no
    /// flattened field serde reads, and an adjacently tagged enum, whose tag and
    /// content it names. An internally tagged enum reads through
    /// `deserialize_any`, and a struct with a flattened field serde reads,
    /// `PhantomData` included, through `deserialize_map`, both of which only
    /// borrow the keys. A shape that is not `Flatten` claims nothing.
    #[test]
    fn only_a_shape_serde_reads_by_name_claims_closed_flatten() {
        assert!(claims_closed_flatten(quote::quote!(
            struct Audit {
                at: String,
            }
        )));
        assert!(claims_closed_flatten(quote::quote!(
            #[serde(tag = "kind", content = "value")]
            enum Payload {
                Number(u32),
                Named { width: u32 },
                Nothing,
            }
        )));

        assert!(!claims_closed_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Shape {
                Circle { radius: f64 },
                Point,
            }
        )));
        assert!(!claims_closed_flatten(quote::quote!(
            #[serde(tag = "kind")]
            enum Marker {
                On,
                Off,
            }
        )));

        // A flattened field serde reads makes serde read the struct as a map;
        // one it skips both ways, however that is spelt, leaves it read by
        // name.
        assert!(!claims_closed_flatten(quote::quote!(
            struct Stamped {
                id: u64,
                #[serde(flatten)]
                audit: Audit,
            }
        )));
        assert!(!claims_closed_flatten(quote::quote!(
            struct Stamped<T> {
                id: u64,
                #[serde(flatten)]
                marker: PhantomData<T>,
            }
        )));
        assert!(claims_closed_flatten(quote::quote!(
            struct Stamped {
                id: u64,
                #[serde(flatten)]
                #[serde(skip)]
                audit: Audit,
            }
        )));
        assert!(claims_closed_flatten(quote::quote!(
            struct Stamped {
                id: u64,
                #[serde(flatten)]
                #[serde(skip_serializing, skip_deserializing)]
                audit: Audit,
            }
        )));

        // A struct's container tag is a member serde writes beside its fields
        // and never names among them, so it takes no tag key out of a closed
        // parent's buffer.
        assert!(!claims_closed_flatten(quote::quote!(
            #[serde(tag = "type")]
            struct Tagged {
                a: u8,
            }
        )));

        // Not `Flatten` at all, so not the narrower claim either.
        assert!(!claims_closed_flatten(quote::quote!(
            enum Event {
                Created { at: String },
            }
        )));
        assert!(!claims_closed_flatten(quote::quote!(
            #[serde(deny_unknown_fields)]
            struct Audit {
                at: String,
            }
        )));
        assert!(!claims_closed_flatten(quote::quote!(
            #[serde(transparent)]
            struct Labels {
                inner: Audit,
            }
        )));
    }

    /// How many `ClosedFlatten` witnesses the expansion asserts for the input.
    fn closed_flatten_witnesses_in(declaration: TokenStream2) -> usize {
        let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
        let expansion = expand_inner(&input).expect("the case itself must expand");
        expansion
            .to_string()
            .matches("is_closed_flattenable ::")
            .count()
    }

    /// A flattened field of an object `deny_unknown_fields` closes is bounded by
    /// `ClosedFlatten` beside `Flatten`, and one of an open object by `Flatten`
    /// alone.
    ///
    /// `Flatten` stays asserted so a type that is not flattenable at all is
    /// refused with its own reason rather than only for how serde reads it.
    /// One row per object serde closes: a struct, and a struct variant under
    /// each tagging. An internally tagged newtype variant's payload stays
    /// bounded by `Flatten` alone, since serde hands it the keys beside the tag
    /// whatever the attribute says, and its tag-only object stays open.
    #[test]
    fn a_closed_object_bounds_its_flattened_fields_by_closed_flatten() {
        let witnesses = |declaration: TokenStream2| {
            (
                closed_flatten_witnesses_in(declaration.clone()),
                flatten_witnesses_in(declaration),
            )
        };

        assert_eq!(
            witnesses(quote::quote!(
                #[serde(deny_unknown_fields)]
                struct Thing {
                    id: u64,
                    #[serde(flatten)]
                    audit: Audit,
                }
            )),
            (1, 1)
        );
        // Open, the same struct keeps only the wider bound, so the case isolates the
        // attribute rather than the shape.
        assert_eq!(
            witnesses(quote::quote!(
                struct Thing {
                    id: u64,
                    #[serde(flatten)]
                    audit: Audit,
                }
            )),
            (0, 1)
        );

        for declaration in [
            quote::quote!(
                #[serde(deny_unknown_fields, tag = "kind")]
                enum Event {
                    Created {
                        #[serde(flatten)]
                        audit: Audit,
                    },
                }
            ),
            quote::quote!(
                #[serde(deny_unknown_fields, tag = "kind", content = "data")]
                enum Event {
                    Created {
                        #[serde(flatten)]
                        audit: Audit,
                    },
                }
            ),
            quote::quote!(
                #[serde(deny_unknown_fields)]
                enum Event {
                    Created {
                        #[serde(flatten)]
                        audit: Audit,
                    },
                }
            ),
        ] {
            assert_eq!(witnesses(declaration), (1, 1));
        }

        assert_eq!(
            witnesses(quote::quote!(
                #[serde(deny_unknown_fields, tag = "kind")]
                enum Event {
                    Raw(Audit),
                }
            )),
            (0, 1)
        );
        // A transparent struct is its one field's value, with no object for the
        // attribute to close, so its flattened field keeps `Flatten` alone.
        assert_eq!(
            witnesses(quote::quote!(
                #[serde(transparent, deny_unknown_fields)]
                struct Wrapper {
                    #[serde(flatten)]
                    audit: Audit,
                }
            )),
            (0, 1)
        );
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
