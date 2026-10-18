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
//! grammar without a row fails the build, and the sweep in the crate root's
//! `tests.rs` fails one added to a file no counter names.

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
                "a generic type naming itself in a field it describes",
                quote::quote!(
                    struct Node<T> {
                        value: T,
                        next: Option<Box<Node<T>>>,
                    }
                ),
                "refers to itself",
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

    /// The values a `content_*` annotation refuses; a function of its own for
    /// the reason `serde_ledger` gives.
    fn content_ledger() -> Vec<Case> {
        vec![
            case(
                "a `content_encoding` no RFC JSON Schema points to defines",
                quote::quote!(
                    struct Upload {
                        #[schema(content_encoding = "base-64")]
                        torrent: String,
                    }
                ),
                "is not an encoding `contentEncoding` names",
            ),
            case(
                "a `content_media_type` that is not a media type",
                quote::quote!(
                    struct Upload {
                        #[schema(content_media_type = "bittorrent")]
                        torrent: String,
                    }
                ),
                "is not a media type",
            ),
        ]
    }

    /// Where `#[schema(as = T)]` cannot describe a field; a function of its
    /// own for the reason `serde_ledger` gives.
    fn described_as_ledger() -> Vec<Case> {
        vec![
            case(
                "`as` written twice on one field, in two lists",
                quote::quote!(
                    struct Digest {
                        #[schema(as = String)]
                        #[schema(min_length = 1, as = Vec<u8>)]
                        hash: Hash,
                    }
                ),
                "may appear once per field",
            ),
            case(
                "`as` on a flattened field, which is no one value",
                quote::quote!(
                    struct Order {
                        #[serde(flatten)]
                        #[schema(as = Line)]
                        line: Item,
                    }
                ),
                "a flattened field is no one value",
            ),
            case(
                "`as` on an internally tagged newtype variant's payload, written beside the tag",
                quote::quote!(
                    #[serde(tag = "kind")]
                    enum Event {
                        Created(#[schema(as = Summary)] Record),
                    }
                ),
                "an internally tagged newtype variant's payload is no one value",
            ),
            case(
                "`as` on a `PhantomData`, which serde writes as `null` whatever it is described as",
                quote::quote!(
                    struct Marker<T> {
                        #[schema(as = String)]
                        marker: PhantomData<T>,
                    }
                ),
                "a `PhantomData` is the `null`",
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
                "a `#[serde(field_identifier)]` enum's trailing newtype variant, a catch-all serde \
                 reads but the description cannot state",
                quote::quote!(
                    #[serde(field_identifier)]
                    enum Field {
                        Id,
                        Other(String),
                    }
                ),
                "`#[serde(field_identifier)]` reads every identifier",
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
            case(
                "a tagged enum whose every variant serde skips both ways",
                quote::quote!(
                    #[serde(tag = "kind")]
                    enum Signal {
                        #[serde(skip)]
                        Start,
                    }
                ),
                "no branch for its `oneOf`",
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
            case(
                "a constraint written twice, across two lists, when the second would replace \
                 the first",
                quote::quote!(
                    struct Order {
                        #[schema(minimum = 1)]
                        #[schema(minimum = 2)]
                        total: u32,
                    }
                ),
                "`minimum` is already set",
            ),
            // One site, which says why the check cannot enforce the pattern:
            // what it is, with the feature, or that there is no engine.
            #[cfg(feature = "pattern")]
            case(
                "a `pattern` the check cannot enforce",
                quote::quote!(
                    struct Order {
                        #[schema(pattern = "a(?=b)")]
                        code: String,
                    }
                ),
                "lookaround and backreferences",
            ),
            #[cfg(not(feature = "pattern"))]
            case(
                "a `pattern` without the `pattern` feature",
                quote::quote!(
                    struct Order {
                        #[schema(pattern = "^[a-z]+$")]
                        code: String,
                    }
                ),
                "only under its `pattern` feature",
            ),
        ]
    }

    /// The refusals a struct's own `#[serde(tag = "...")]` raises, where
    /// serde writes the tag beside the fields and no schema is true of both
    /// directions.
    ///
    /// A fifth function, since `serde_ledger` is at the length Clippy accepts.
    fn tag_ledger() -> Vec<Case> {
        vec![
            case(
                "deny_unknown_fields on a tagged struct",
                quote::quote!(
                    #[serde(tag = "type", deny_unknown_fields)]
                    struct Stamp {
                        at: u64,
                    }
                ),
                "refuses every document the struct writes",
            ),
            case(
                "a field named as its struct's tag",
                quote::quote!(
                    #[serde(tag = "kind")]
                    struct Stamp {
                        at: u64,
                        kind: String,
                    }
                ),
                "is also this struct's",
            ),
        ]
    }

    /// The refusal a split `rename` raises where serde writes a member under
    /// one name and reads it under another.
    ///
    /// A sixth function, since a split rename is neither a skip nor a tag. A
    /// container `rename_all` whose sides differ is refused beside it.
    fn rename_ledger() -> Vec<Case> {
        vec![
            case(
                "a split `rename` whose sides differ on a field serde writes and reads",
                quote::quote!(
                    struct Stamp {
                        #[serde(rename(serialize = "a", deserialize = "b"))]
                        at: u64,
                    }
                ),
                "and reads it as",
            ),
            case(
                "a split container `rename_all` whose sides differ",
                quote::quote!(
                    #[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
                    struct Stamp {
                        created_at: u64,
                    }
                ),
                "a split `rename_all` whose sides differ",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(content_ledger(), expand_inner);
        each_case_is_refused(described_as_ledger(), expand_inner);
        each_case_is_refused(serde_ledger(), expand_inner);
        each_case_is_refused(variant_ledger(), expand_inner);
        each_case_is_refused(field_ledger(), expand_inner);
        each_case_is_refused(tag_ledger(), expand_inner);
        each_case_is_refused(rename_ledger(), expand_inner);
    }

    /// A qualified key matches no constraint, so it reaches the unknown-key
    /// refusal; and every key, flags and `open` included, is written once.
    #[test]
    fn a_constraint_refuses_a_qualified_or_repeated_key() {
        each_case_is_refused(
            vec![
                case(
                    "a qualified constraint key",
                    quote::quote!(
                        struct Order {
                            #[schema(kynos::minimum = 1)]
                            total: u32,
                        }
                    ),
                    "`kynos::minimum` is not part of the `#[schema(...)]` grammar",
                ),
                case(
                    "a count written twice in one list",
                    quote::quote!(
                        struct Order {
                            #[schema(max_length = 8, max_length = 9)]
                            code: String,
                        }
                    ),
                    "`max_length` is already set",
                ),
                case(
                    "a flag written twice",
                    quote::quote!(
                        struct Order {
                            #[schema(unique_items, unique_items)]
                            tags: Vec<String>,
                        }
                    ),
                    "`unique_items` is already set",
                ),
                case(
                    "open written twice",
                    quote::quote!(
                        struct Order {
                            #[serde(flatten)]
                            #[schema(open, open)]
                            extra: BTreeMap<String, String>,
                        }
                    ),
                    "`open` is already set",
                ),
            ],
            expand_inner,
        );
    }

    #[test]
    fn every_schema_diagnostic_has_a_case() {
        // The derive raises the union refusal in `schema.rs` and every other
        // rule in a module under `schema/refusals/`.
        every_diagnostic_has_a_case(
            "schema.rs` and `schema/refusals/",
            &[
                include_str!("schema.rs"),
                include_str!("schema/refusals.rs"),
                include_str!("schema/refusals/grammar.rs"),
                include_str!("schema/refusals/naming.rs"),
                include_str!("schema/refusals/object_keys.rs"),
                include_str!("schema/refusals/recursion.rs"),
                include_str!("schema/refusals/skips.rs"),
                include_str!("schema/refusals/wire_form.rs"),
            ]
            .concat(),
            ledger().len()
                + content_ledger().len()
                + described_as_ledger().len()
                + serde_ledger().len()
                + variant_ledger().len()
                + field_ledger().len()
                + tag_ledger().len()
                + rename_ledger().len(),
        );
    }

    /// The `content_encoding` the derive accepts is exactly the set JSON
    /// Schema points to, lowercase, and a `content_media_type` is RFC 9110's
    /// `media-type`, parameters and their quoted strings included.
    ///
    /// Beside the ledger rather than in it: its rows prove each site fires,
    /// and these prove where each grammar's edge is.
    #[test]
    fn a_content_annotation_is_held_to_its_grammar() {
        let declare = |key: &str, value: &str| -> DeriveInput {
            let key = syn::Ident::new(key, proc_macro2::Span::call_site());
            syn::parse2(quote::quote!(
                struct Upload {
                    #[schema(#key = #value)]
                    torrent: String,
                }
            ))
            .expect("the declaration parses")
        };

        for encoding in [
            "base16",
            "base32",
            "base32hex",
            "base64",
            "base64url",
            "quoted-printable",
        ] {
            assert!(
                expand_inner(&declare("content_encoding", encoding)).is_ok(),
                "{encoding:?} is an encoding JSON Schema names"
            );
        }
        for encoding in [
            "", "BASE64", "base-64", "7bit", "binary", "identity", "gzip",
        ] {
            assert!(
                expand_inner(&declare("content_encoding", encoding)).is_err(),
                "{encoding:?} is no encoding JSON Schema names"
            );
        }

        for media_type in [
            "image/png",
            "application/x-bittorrent",
            "application/vnd.example+json",
            "TEXT/Plain",
            "text/plain;charset=utf-8",
            "text/plain ; charset=utf-8",
            "text/plain;",
            "text/plain; ; charset=utf-8",
            "text/plain; title=\"a; b \\\"c\\\"\"",
            "text/plain; a=b\t;c=d",
        ] {
            assert!(
                expand_inner(&declare("content_media_type", media_type)).is_ok(),
                "{media_type:?} is a media type"
            );
        }
        for media_type in [
            "",
            "image",
            "image/",
            "/png",
            "image/png/x",
            " image/png",
            "image/png ",
            "image /png",
            "image/p ng",
            "text/plain; charset",
            "text/plain; =utf-8",
            "text/plain; charset=",
            "text/plain; charset=utf 8",
            "text/plain; title=\"open",
            "text/plain; title=\"a\"b",
            "text/plain; title=\"a\\",
            "text/plain; title=\"a\u{7}\"",
            "text/plain; title=\"a\\\u{0}\"",
            "text/plain, image/png",
        ] {
            assert!(
                expand_inner(&declare("content_media_type", media_type)).is_err(),
                "{media_type:?} is not a media type"
            );
        }
    }

    /// `Self` names the type as surely as its identifier does, in a variant as
    /// in a field.
    ///
    /// Beside the ledger rather than in it: the ledger's row proves the site
    /// fires, and this proves `Self` and an enum reach it.
    #[test]
    fn a_generic_enum_naming_itself_as_self_is_refused() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            enum Tree<T> {
                Leaf(T),
                Branch { children: Vec<Self> },
            }
        ))
        .expect("the case itself must parse");

        let Err(error) = expand_inner(&input) else {
            panic!("a generic enum naming itself as `Self` must be refused");
        };
        assert!(
            error.to_string().contains("refers to itself"),
            "refused with another diagnostic: {error}"
        );
    }

    /// A member's `#[schema(as = T)]` is what its schema resolves, so a generic
    /// type naming itself there recurses as surely as through the member's own
    /// type, and a member described as something else no longer does.
    #[test]
    fn a_generic_type_is_judged_by_what_its_members_are_described_as() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            struct Node<T> {
                value: T,
                #[schema(as = Vec<Node<T>>)]
                children: Children,
            }
        ))
        .expect("the case itself must parse");
        let Err(error) = expand_inner(&input) else {
            panic!("a generic type described as itself must be refused");
        };
        assert!(
            error.to_string().contains("refers to itself"),
            "refused with another diagnostic: {error}"
        );

        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            struct Node<T> {
                value: T,
                #[schema(as = Vec<String>)]
                children: Vec<Node<T>>,
            }
        ))
        .expect("the case itself must parse");
        assert!(
            expand_inner(&input).is_ok(),
            "a member described as another type was judged by its own"
        );
    }

    /// A self-reference is found inside every type the walk descends, not only
    /// between angle brackets: a tuple, an array, a slice, parentheses, a
    /// reference, a raw pointer, an associated-type binding, and the invisible
    /// group a `macro_rules!` `$ty` fragment arrives in.
    #[test]
    fn a_generic_type_naming_itself_inside_a_nested_type_is_refused() {
        let grouped = proc_macro2::Group::new(proc_macro2::Delimiter::None, quote::quote!(Node<T>));
        for declaration in [
            quote::quote!(
                struct Node<T> {
                    value: T,
                    pairs: Vec<(String, Node<T>)>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    children: Box<[Self; 2]>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    children: Box<[Node<T>]>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    next: Option<Box<(Self)>>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    next: Option<&'static Self>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    next: Option<*const Node<T>>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    next: Wrapper<Item = Self>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    next: Option<Box<#grouped>>,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");
            let Err(error) = expand_inner(&input) else {
                panic!("a generic type naming itself inside a nested type must be refused");
            };
            assert!(
                error.to_string().contains("refers to itself"),
                "refused with another diagnostic: {error}"
            );
        }
    }

    /// Only a generic type naming itself where its schema descends is refused:
    /// a concrete type is named and so `$ref`s itself, and a member serde skips
    /// both ways, a `PhantomData`, a path qualified to another type of the
    /// same identifier, or an associated type projected from it is never
    /// described as this type.
    #[test]
    fn a_self_reference_the_schema_never_descends_into_is_accepted() {
        for declaration in [
            quote::quote!(
                struct Node<T: Tr> {
                    value: T,
                    by_self: <Self as Tr>::Out,
                    by_name: Vec<<Node<T> as Tr>::Out>,
                    by_trait: <T as Tr<Node<T>>>::Out,
                }
            ),
            quote::quote!(
                struct Node {
                    next: Option<Box<Node>>,
                }
            ),
            quote::quote!(
                struct Node<T> {
                    value: T,
                    #[serde(skip)]
                    next: Option<Box<Node<T>>>,
                    marker: core::marker::PhantomData<Node<T>>,
                    other: other::Node<T>,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");
            if let Err(error) = expand_inner(&input) {
                panic!("a self-reference the schema never descends into was refused: {error}");
            }
        }
    }

    /// A split container `rename_all` naming one side only leaves the other
    /// direction on the identifiers, so its sides differ too.
    ///
    /// Beside the ledger rather than in it: the ledger's row proves the site
    /// fires, and this proves an absent side counts as a differing one.
    #[test]
    fn a_split_rename_all_naming_one_side_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "a split container `rename_all` naming its serialize side only",
                    quote::quote!(
                        #[serde(rename_all(serialize = "camelCase"))]
                        struct Stamp {
                            created_at: u64,
                        }
                    ),
                    "a split `rename_all` whose sides differ",
                ),
                case(
                    "a split container `rename_all` naming its deserialize side only",
                    quote::quote!(
                        #[serde(rename_all(deserialize = "camelCase"))]
                        struct Stamp {
                            created_at: u64,
                        }
                    ),
                    "a split `rename_all` whose sides differ",
                ),
            ],
            expand_inner,
        );
    }

    /// A split `rename` is refused on every member serde both writes and
    /// reads, not only on a struct's field: a variant, and a field of a
    /// variant serde writes. A `rename` giving one side is split too: its other
    /// side is the identifier without its `r#` under `rename_all`.
    ///
    /// Beside the ledger rather than in it: the ledger's row proves the site
    /// fires, and these prove each member reaches it.
    #[test]
    fn a_split_rename_is_refused_on_every_member_serde_writes_and_reads() {
        each_case_is_refused(
            vec![
                case(
                    "a split `rename` whose sides differ on a variant serde writes and reads",
                    quote::quote!(
                        enum Change {
                            Now,
                            #[serde(rename(serialize = "x", deserialize = "y"))]
                            Queued,
                        }
                    ),
                    "serde writes this variant as `x` and reads it as `y`",
                ),
                case(
                    "a split `rename` whose sides differ on a field of a variant serde writes",
                    quote::quote!(
                        enum Change {
                            Now(u64),
                            Queued {
                                #[serde(rename(serialize = "a", deserialize = "b"))]
                                at: u64,
                            },
                        }
                    ),
                    "serde writes this field as `a` and reads it as `b`",
                ),
                case(
                    "a split `rename` whose written side no `alias` covers",
                    quote::quote!(
                        struct Stamp {
                            #[serde(rename(serialize = "a", deserialize = "b"), alias = "c")]
                            at: u64,
                        }
                    ),
                    "serde writes this field as `a` and reads it as `b`",
                ),
                case(
                    "a split `rename` whose written side is an `alias` an earlier variant claims",
                    quote::quote!(
                        enum Change {
                            #[serde(alias = "x")]
                            Now,
                            #[serde(rename(serialize = "x", deserialize = "y"), alias = "x")]
                            Queued,
                        }
                    ),
                    "serde writes this variant as `x` and reads it as `y`",
                ),
                case(
                    "a `rename` giving only the serialize side of a field",
                    quote::quote!(
                        #[serde(rename_all = "camelCase")]
                        struct Stamp {
                            #[serde(rename(serialize = "a"))]
                            r#type_name: u64,
                        }
                    ),
                    "serde writes this field as `a` and reads it as `typeName`",
                ),
                case(
                    "a `rename` giving only the deserialize side of a field",
                    quote::quote!(
                        struct Stamp {
                            #[serde(rename(deserialize = "b"))]
                            r#type: u64,
                        }
                    ),
                    "serde writes this field as `type` and reads it as `b`",
                ),
                case(
                    "a `rename` giving only the deserialize side of a variant",
                    quote::quote!(
                        #[serde(rename_all = "snake_case")]
                        enum Change {
                            Now,
                            #[serde(rename(deserialize = "b"))]
                            QueuedLater,
                        }
                    ),
                    "serde writes this variant as `queued_later` and reads it as `b`",
                ),
            ],
            expand_inner,
        );
    }

    /// A split `rename` is refused only where serde uses both sides and never
    /// reads the written one, so each member one schema is true of is accepted:
    /// sides that agree, a field or a variant whose written side is also an
    /// `alias`, a field serde only reads or only writes, a field of a variant
    /// serde never writes, a flattened field, a transparent struct's field,
    /// and a variant serde only reads.
    #[test]
    fn a_split_rename_one_schema_is_true_of_is_accepted() {
        for declaration in [
            quote::quote!(
                struct Stamp {
                    #[serde(rename(serialize = "a", deserialize = "a"))]
                    at: u64,
                }
            ),
            quote::quote!(
                struct Stamp {
                    #[serde(rename(serialize = "a", deserialize = "b"), alias = "a")]
                    at: u64,
                }
            ),
            quote::quote!(
                enum Change {
                    Now,
                    #[serde(rename(serialize = "x", deserialize = "y"), alias = "x")]
                    Queued,
                }
            ),
            quote::quote!(
                struct Stamp {
                    #[serde(skip_serializing, default, rename(serialize = "a", deserialize = "b"))]
                    at: u64,
                }
            ),
            quote::quote!(
                struct Stamp {
                    #[serde(skip_deserializing, rename(serialize = "a", deserialize = "b"))]
                    at: u64,
                }
            ),
            quote::quote!(
                enum Change {
                    Now(u64),
                    #[serde(skip_serializing)]
                    Queued {
                        #[serde(rename(serialize = "a", deserialize = "b"))]
                        at: u64,
                    },
                }
            ),
            quote::quote!(
                struct Stamp {
                    at: u64,
                    #[serde(flatten, rename(serialize = "a", deserialize = "b"))]
                    audit: Audit,
                }
            ),
            quote::quote!(
                #[serde(transparent)]
                struct Stamp {
                    #[serde(rename(serialize = "a", deserialize = "b"))]
                    at: u64,
                }
            ),
            quote::quote!(
                enum Change {
                    Now,
                    #[serde(skip_serializing, rename(serialize = "a", deserialize = "b"))]
                    Queued,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");
            if let Err(error) = expand_inner(&input) {
                panic!("a split rename with a true schema was refused: {error}");
            }
        }
    }

    /// A struct variant's field is judged by the name serde gives it under the
    /// variant's field rule, not the enum's `rename_all`: a one-sided `rename`
    /// the rule's default matches is one name, and one it does not match is
    /// two.
    #[test]
    fn a_split_rename_is_judged_by_the_variants_field_rule() {
        let accepted: syn::DeriveInput = syn::parse2(quote::quote!(
            enum Change {
                #[serde(rename_all = "camelCase")]
                Created {
                    #[serde(rename(serialize = "userId"))]
                    user_id: u64,
                },
            }
        ))
        .expect("the case itself must parse");
        if let Err(error) = expand_inner(&accepted) {
            panic!("a field serde writes and reads as `userId` was refused: {error}");
        }

        each_case_is_refused(
            vec![case(
                "a one-sided `rename` the enum's `rename_all_fields` does not match",
                quote::quote!(
                    #[serde(rename_all_fields = "camelCase")]
                    enum Change {
                        Created {
                            #[serde(rename(deserialize = "user_id"))]
                            user_id: u64,
                        },
                    }
                ),
                "serde writes this field as `userId` and reads it as `user_id`",
            )],
            expand_inner,
        );
    }

    /// A field rule serde reads in split form is refused where its sides
    /// differ, as a container `rename_all` is, where it names a field serde
    /// both writes and reads: the enum's `rename_all_fields` over a struct
    /// variant, and the own `rename_all` of a variant serde writes and reads.
    /// Sides that agree are the one style.
    #[test]
    fn a_split_field_rule_whose_sides_differ_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "a split `rename_all_fields` whose sides differ",
                    quote::quote!(
                        #[serde(rename_all_fields(
                            serialize = "camelCase",
                            deserialize = "snake_case"
                        ))]
                        enum Change {
                            Created { user_id: u64 },
                        }
                    ),
                    "a split `rename_all_fields` whose sides differ",
                ),
                case(
                    "a variant's split `rename_all` naming one side only",
                    quote::quote!(
                        enum Change {
                            #[serde(rename_all(serialize = "camelCase"))]
                            Created { user_id: u64 },
                        }
                    ),
                    "a split `rename_all` whose sides differ gives every field of this variant",
                ),
                case(
                    "a split `rename_all_fields` reaching a side a written variant's rule leaves",
                    quote::quote!(
                        #[serde(rename_all_fields(
                            serialize = "camelCase",
                            deserialize = "kebab-case"
                        ))]
                        enum Change {
                            A,
                            #[serde(rename_all(serialize = "snake_case"))]
                            B {
                                user_id: u8,
                            },
                        }
                    ),
                    "a split `rename_all_fields` whose sides differ",
                ),
            ],
            expand_inner,
        );

        let accepted: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(rename_all_fields(serialize = "camelCase", deserialize = "camelCase"))]
            enum Change {
                #[serde(rename_all(serialize = "kebab-case", deserialize = "kebab-case"))]
                Created { user_id: u64 },
            }
        ))
        .expect("the case itself must parse");
        if let Err(error) = expand_inner(&accepted) {
            panic!("a split field rule whose sides agree was refused: {error}");
        }
    }

    /// A split field rule is refused only where it names a field serde both
    /// writes and reads: a variant's own rule on a variant serde skips both
    /// ways or only reads, or on a unit or tuple variant, which has no named
    /// field, and an enum's `rename_all_fields` that reaches no
    /// struct variant, one with no field or every one under its own rule, are
    /// accepted, as serde accepts them.
    #[test]
    fn a_split_field_rule_no_field_is_named_both_ways_by_is_accepted() {
        let cases = [
            (
                "a variant serde skips both ways",
                quote::quote!(
                    enum Change {
                        A,
                        #[serde(skip, rename_all(serialize = "camelCase"))]
                        B {
                            user_id: u8,
                        },
                    }
                ),
            ),
            (
                "a variant serde only reads, under its rule's deserialize side",
                quote::quote!(
                    enum Change {
                        A,
                        #[serde(skip_serializing, rename_all(deserialize = "camelCase"))]
                        B {
                            user_id: u8,
                        },
                    }
                ),
            ),
            (
                "a unit variant serde writes and reads",
                quote::quote!(
                    enum Change {
                        #[serde(rename_all(serialize = "camelCase"))]
                        A,
                    }
                ),
            ),
            (
                "a tuple variant serde writes and reads",
                quote::quote!(
                    enum Change {
                        #[serde(rename_all(serialize = "camelCase", deserialize = "kebab-case"))]
                        A(u8),
                    }
                ),
            ),
            (
                "an enum's `rename_all_fields` over unit variants alone",
                quote::quote!(
                    #[serde(rename_all_fields(serialize = "camelCase"))]
                    enum UnitsOnly {
                        A,
                        B,
                    }
                ),
            ),
            (
                "an enum's `rename_all_fields` over a variant under its own rule",
                quote::quote!(
                    #[serde(rename_all_fields(serialize = "camelCase"))]
                    enum Change {
                        #[serde(rename_all = "kebab-case")]
                        B { user_id: u8 },
                    }
                ),
            ),
        ];
        for (label, tokens) in cases {
            let input: syn::DeriveInput = syn::parse2(tokens).expect("the case itself must parse");
            if let Err(error) = expand_inner(&input) {
                panic!("{label} was refused: {error}");
            }
        }
    }

    /// serde fills each side a variant's own `rename_all` leaves from the
    /// enum's `rename_all_fields`, so a one-sided rule that side completes to
    /// the same style is one name, described under it.
    #[test]
    fn a_one_sided_variant_rule_completed_by_rename_all_fields_is_accepted() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(rename_all_fields = "camelCase")]
            enum Change {
                #[serde(rename_all(serialize = "camelCase"))]
                Created { user_id: u8 },
            }
        ))
        .expect("the case itself must parse");
        let expansion = match expand_inner(&input) {
            Ok(expansion) => expansion.to_string(),
            Err(error) => panic!("a field serde writes and reads as `userId` was refused: {error}"),
        };
        assert!(
            expansion.contains("\"userId\"") && !expansion.contains("\"user_id\""),
            "the field is described under a name other than `userId`: {expansion}"
        );
    }

    /// serde resolves each side of a variant's field rule as its own
    /// `rename_all`, else the enum's `rename_all_fields`, so a split
    /// `rename_all_fields` is judged per variant by the sides that variant
    /// resolves: one that names no field two ways is accepted, and a variant
    /// serde only reads names its fields by the deserialize side.
    #[test]
    fn a_split_rename_all_fields_is_judged_by_the_sides_each_variant_resolves() {
        let cases = [
            (
                "a written variant whose own rule matches the side it leaves",
                quote::quote!(
                    #[serde(rename_all_fields(
                        serialize = "camelCase",
                        deserialize = "snake_case"
                    ))]
                    enum E {
                        #[serde(rename_all(serialize = "snake_case"))]
                        V { user_id: u8 },
                    }
                ),
                "\"user_id\"",
                ["\"userId\"", "\"user-id\""],
            ),
            (
                "a read-only variant whose own rule leaves the deserialize side",
                quote::quote!(
                    #[serde(rename_all_fields(
                        serialize = "camelCase",
                        deserialize = "kebab-case"
                    ))]
                    enum E {
                        A,
                        #[serde(skip_serializing, rename_all(serialize = "snake_case"))]
                        V {
                            user_id: u8,
                        },
                    }
                ),
                "\"user-id\"",
                ["\"userId\"", "\"user_id\""],
            ),
            (
                "a read-only variant with no rule of its own",
                quote::quote!(
                    #[serde(rename_all_fields(
                        serialize = "camelCase",
                        deserialize = "kebab-case"
                    ))]
                    enum E {
                        A,
                        #[serde(skip_serializing)]
                        V {
                            user_id: u8,
                        },
                    }
                ),
                "\"user-id\"",
                ["\"userId\"", "\"user_id\""],
            ),
        ];
        for (label, tokens, named, unnamed) in cases {
            let input: syn::DeriveInput = syn::parse2(tokens).expect("the case itself must parse");
            let expansion = match expand_inner(&input) {
                Ok(expansion) => expansion.to_string(),
                Err(error) => panic!("{label} was refused: {error}"),
            };
            assert!(
                expansion.contains(named) && unnamed.iter().all(|name| !expansion.contains(name)),
                "{label}: the field is described under a name other than {named}: {expansion}"
            );
        }
    }

    /// A variant serde only reads claims the name it is read under, so its
    /// split `rename` shadows an earlier variant by the deserialize side
    /// alone: a serialize side naming the earlier variant is never used, and a
    /// deserialize side naming it leaves the variant unreadable.
    #[test]
    fn a_read_only_variant_is_shadowed_by_the_side_serde_reads() {
        let accepted: syn::DeriveInput = syn::parse2(quote::quote!(
            enum Change {
                Now,
                #[serde(skip_serializing, rename(serialize = "Now", deserialize = "later"))]
                Queued,
            }
        ))
        .expect("the case itself must parse");
        if let Err(error) = expand_inner(&accepted) {
            panic!("a variant serde never writes under a claimed name was refused: {error}");
        }

        each_case_is_refused(
            vec![case(
                "a variant serde only reads, under an earlier variant's name",
                quote::quote!(
                    enum Change {
                        Now,
                        #[serde(skip_serializing, rename(serialize = "later", deserialize = "Now"))]
                        Queued,
                    }
                ),
                "serde reads `Now`, this variant's own name",
            )],
            expand_inner,
        );
    }

    /// A tag is refused only where no schema is true of it, so each shape
    /// that has one is accepted: an open struct, a transparent one serde
    /// writes without its tag, and a field under the tag's name serde never
    /// writes or reads there.
    #[test]
    fn a_tag_on_a_struct_with_a_true_schema_is_accepted() {
        for declaration in [
            quote::quote!(
                #[serde(tag = "type")]
                struct Stamp {
                    at: u64,
                }
            ),
            quote::quote!(
                #[serde(transparent, tag = "type", deny_unknown_fields)]
                struct Stamp {
                    at: u64,
                }
            ),
            quote::quote!(
                #[serde(tag = "kind")]
                struct Stamp {
                    at: u64,
                    #[serde(skip)]
                    kind: String,
                }
            ),
            quote::quote!(
                #[serde(tag = "kind")]
                struct Stamp {
                    at: u64,
                    #[serde(skip_deserializing, alias = "kind")]
                    seen: u64,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");
            if let Err(error) = expand_inner(&input) {
                panic!("a tag with a true schema was refused: {error}");
            }
        }
    }

    /// A flattened field's own name is never written, so one named as the tag
    /// is accepted. The keys its type writes are not checked: the derive
    /// cannot see them at expansion time, the limit serde's own check of an
    /// enum's internal tag has too, so a flattened type writing the tag's key
    /// is accepted with a schema serde's document does not meet.
    #[test]
    fn a_flattened_field_is_exempt_from_the_tag_whatever_its_type_writes() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(tag = "kind")]
            struct Stamp {
                at: u64,
                #[serde(flatten)]
                kind: Audit,
            }
        ))
        .expect("the case itself must parse");
        if let Err(error) = expand_inner(&input) {
            panic!("a flattened field named as the tag was refused: {error}");
        }
    }

    /// A field collides with its struct's tag in either direction alone: read
    /// under the tag's name through an `alias`, or written under it by one
    /// serde never reads.
    #[test]
    fn a_field_under_the_tags_name_is_refused_in_either_direction() {
        each_case_is_refused(
            vec![
                case(
                    "a field serde reads under the tag's name through an `alias`",
                    quote::quote!(
                        #[serde(tag = "kind")]
                        struct Stamp {
                            at: u64,
                            #[serde(alias = "kind")]
                            seen: u64,
                        }
                    ),
                    "is also this struct's",
                ),
                case(
                    "a field serde writes under the tag's name and never reads",
                    quote::quote!(
                        #[serde(tag = "kind")]
                        struct Stamp {
                            at: u64,
                            #[serde(skip_deserializing)]
                            kind: String,
                        }
                    ),
                    "is also this struct's",
                ),
                case(
                    "a raw identifier serde names as the tag",
                    quote::quote!(
                        #[serde(tag = "type")]
                        struct Stamp {
                            at: u64,
                            r#type: String,
                        }
                    ),
                    "is also this struct's",
                ),
                case(
                    "a field serde writes under the tag's name by a split `rename`",
                    quote::quote!(
                        #[serde(tag = "kind")]
                        struct Stamp {
                            at: u64,
                            #[serde(
                                skip_deserializing,
                                rename(serialize = "kind", deserialize = "k")
                            )]
                            x: String,
                        }
                    ),
                    "is also this struct's",
                ),
                case(
                    "a field serde reads under the tag's name by a split `rename`",
                    quote::quote!(
                        #[serde(tag = "kind")]
                        struct Stamp {
                            at: u64,
                            #[serde(
                                skip_serializing,
                                default,
                                rename(serialize = "x", deserialize = "kind")
                            )]
                            y: String,
                        }
                    ),
                    "is also this struct's",
                ),
            ],
            expand_inner,
        );
    }

    /// `#[serde(tag = "...")]` on a tuple or unit struct is serde's diagnostic
    /// to raise, for the reason `untagged_on_a_struct_is_left_to_serde` gives:
    /// serde refuses the attribute there, so neither tag refusal applies.
    #[test]
    fn a_tag_on_a_tuple_or_unit_struct_is_left_to_serde() {
        for declaration in [
            quote::quote!(
                #[serde(tag = "t", deny_unknown_fields)]
                struct Pair(u8, u8);
            ),
            quote::quote!(
                #[serde(tag = "t", deny_unknown_fields)]
                struct Marker;
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");
            if let Err(error) = expand_inner(&input) {
                panic!("a struct serde refuses the tag on was refused: {error}");
            }
        }
    }

    /// A tuple variant of an internally tagged enum is serde's diagnostic to
    /// raise: serde refuses the tag there, so its first member is no payload
    /// written beside the tag and `as` on it is not refused as one.
    #[test]
    fn as_on_an_internally_tagged_tuple_variant_is_left_to_serde() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            #[serde(tag = "kind")]
            enum Event {
                Paired(#[schema(as = Summary)] Record, u8),
            }
        ))
        .expect("the case itself must parse");

        if let Err(error) = expand_inner(&input) {
            panic!("a tuple variant serde refuses the tag on was refused: {error}");
        }
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

        if let Err(error) = expand_inner(&input) {
            panic!("a struct serde refuses `untagged` on was refused: {error}");
        }
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
                case(
                    "`deserialize_with` on the read pick of a struct split across two members",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Handle(
                            #[serde(skip_deserializing)] u64,
                            #[serde(skip_serializing, deserialize_with = "from_string")] u64,
                        );
                    ),
                    "`deserialize_with` reads or writes this field",
                ),
                case(
                    "`deserialize_with` before `serialize_with` on the member picked both ways",
                    quote::quote!(
                        #[serde(transparent)]
                        struct Handle(
                            #[serde(
                                deserialize_with = "from_string",
                                serialize_with = "as_string"
                            )]
                            u64,
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

            if let Err(error) = expand_inner(&input) {
                panic!("a struct serde refuses both ways drew a refusal of its own: {error}");
            }
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

    /// A `#[serde(field_identifier)]` enum's catch-all is refused whatever
    /// serde skips.
    ///
    /// serde reads such an enum through its identifier visitor, which takes
    /// the last variant as the fallthrough when it is a newtype or carries
    /// `#[serde(other)]`, and reads no variant's `skip` attributes, so a
    /// skipped catch-all still catches every identifier the enum does not
    /// name.
    #[test]
    fn a_field_identifier_catch_all_serde_skips_is_still_refused() {
        each_case_is_refused(
            vec![
                case(
                    "a skipped trailing newtype variant of a `#[serde(field_identifier)]` enum",
                    quote::quote!(
                        #[serde(field_identifier)]
                        enum Field {
                            Id,
                            #[serde(skip)]
                            Other(String),
                        }
                    ),
                    "`#[serde(field_identifier)]` reads every identifier",
                ),
                case(
                    "a skipped `#[serde(other)]` variant of a `#[serde(field_identifier)]` enum",
                    quote::quote!(
                        #[serde(field_identifier)]
                        enum Field {
                            Id,
                            #[serde(skip)]
                            #[serde(other)]
                            Unknown,
                        }
                    ),
                    "`#[serde(field_identifier)]` reads every identifier",
                ),
            ],
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

    /// An identifier enum's skipped variant is described, never refused.
    ///
    /// serde reads a `#[serde(field_identifier)]` or `variant_identifier`
    /// enum through its identifier visitor, which names every declared
    /// variant and reads no `skip`, `skip_serializing` or
    /// `skip_deserializing`; and it refuses `Serialize` on one, so the enum is
    /// only ever read. Every variant is a name serde accepts, so each is in the
    /// expansion, and rule 28's "refuse to read it back" does not hold.
    #[test]
    fn an_identifier_enums_skipped_variant_is_described() {
        for (declaration, skipped) in [
            (
                quote::quote!(
                    #[serde(field_identifier)]
                    enum Key {
                        Id,
                        #[serde(skip)]
                        Legacy,
                    }
                ),
                "\"Legacy\"",
            ),
            (
                quote::quote!(
                    #[serde(field_identifier)]
                    enum Key {
                        Id,
                        #[serde(skip_serializing)]
                        #[serde(skip_deserializing)]
                        Legacy,
                    }
                ),
                "\"Legacy\"",
            ),
            (
                quote::quote!(
                    #[serde(field_identifier)]
                    enum Key {
                        Id,
                        #[serde(skip_deserializing)]
                        Draft,
                    }
                ),
                "\"Draft\"",
            ),
            (
                quote::quote!(
                    #[serde(variant_identifier)]
                    enum Kind {
                        Web,
                        #[serde(skip)]
                        Fax,
                    }
                ),
                "\"Fax\"",
            ),
            (
                quote::quote!(
                    #[serde(variant_identifier)]
                    enum Kind {
                        Web,
                        #[serde(skip_deserializing)]
                        Fax,
                    }
                ),
                "\"Fax\"",
            ),
        ] {
            let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
            let expansion = match expand_inner(&input) {
                Ok(expansion) => expansion.to_string(),
                Err(error) => panic!("an identifier enum's skipped variant must expand: {error}"),
            };
            assert!(
                expansion.contains(skipped),
                "an identifier enum must describe its skipped variant {skipped}: {expansion}"
            );
        }
    }

    /// serde never writes an identifier enum, so a variant whose own name an
    /// earlier one claims puts nothing on the wire: the name reads as the
    /// earlier variant, and the description lists it once.
    #[test]
    fn an_identifier_enums_shadowed_variant_is_accepted() {
        for declaration in [
            quote::quote!(
                #[serde(field_identifier)]
                enum Key {
                    A,
                    #[serde(skip, rename = "A")]
                    B,
                }
            ),
            quote::quote!(
                #[serde(field_identifier)]
                enum Key {
                    A,
                    #[serde(rename = "A")]
                    B,
                }
            ),
            quote::quote!(
                #[serde(variant_identifier)]
                enum Kind {
                    A,
                    #[serde(skip_deserializing, rename = "A")]
                    B,
                }
            ),
        ] {
            let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
            let expansion = match expand_inner(&input) {
                Ok(expansion) => expansion.to_string(),
                Err(error) => panic!("an identifier enum's shadowed variant must expand: {error}"),
            };
            assert_eq!(
                expansion.matches("\"A\"").count(),
                1,
                "the shared name must be described once: {expansion}"
            );
        }
    }

    /// serde only reads an identifier enum, so a split `rename` or container
    /// `rename_all` gives each variant the one name it is read under: the
    /// deserialize side, which the description lists in place of the serialize
    /// side serde never writes.
    #[test]
    fn an_identifier_enums_split_rename_is_named_by_the_side_serde_reads() {
        for (declaration, read, unwritten) in [
            (
                quote::quote!(
                    #[serde(field_identifier)]
                    enum Key {
                        #[serde(rename(serialize = "written", deserialize = "read"))]
                        A,
                    }
                ),
                "\"read\"",
                "\"written\"",
            ),
            (
                quote::quote!(
                    #[serde(variant_identifier)]
                    enum Kind {
                        #[serde(rename(deserialize = "read"))]
                        Web,
                    }
                ),
                "\"read\"",
                "\"Web\"",
            ),
            (
                quote::quote!(
                    #[serde(
                        field_identifier,
                        rename_all(serialize = "UPPERCASE", deserialize = "kebab-case")
                    )]
                    enum Key {
                        CreatedAt,
                    }
                ),
                "\"created-at\"",
                "\"CREATEDAT\"",
            ),
            (
                quote::quote!(
                    #[serde(rename_all(deserialize = "snake_case"))]
                    #[serde(variant_identifier)]
                    enum Kind {
                        CreatedAt,
                    }
                ),
                "\"created_at\"",
                "\"CreatedAt\"",
            ),
        ] {
            let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
            let expansion = match expand_inner(&input) {
                Ok(expansion) => expansion.to_string(),
                Err(error) => panic!("an identifier enum's split rename must expand: {error}"),
            };
            assert!(
                expansion.contains(read) && !expansion.contains(unwritten),
                "an identifier enum must name its variant {read}, never {unwritten}: {expansion}"
            );
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

    /// Whether the expansion claims `kynos::schema::flatten::Flatten` for the
    /// input.
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

        // An enum with no branch at all describes nothing to flatten. Only
        // the untagged form expands: a tagged one is refused.
        assert!(!claims_flatten(quote::quote!(
            enum Never {}
        )));
    }

    /// A tagged enum with no variant to describe is refused under either
    /// tagging and whether it declares none or skips every one, and an enum
    /// without a tag, the empty `enum`, is not.
    #[test]
    fn a_tagged_enum_with_no_described_variant_is_refused() {
        let refused = [
            quote::quote!(
                #[serde(tag = "kind")]
                enum Never {}
            ),
            quote::quote!(
                #[serde(tag = "kind", content = "value")]
                enum Never {}
            ),
            quote::quote!(
                #[serde(tag = "kind", content = "value")]
                enum Signal {
                    #[serde(skip)]
                    Start,
                    #[serde(skip_serializing, skip_deserializing)]
                    Stop(u64),
                }
            ),
        ];
        for declaration in refused {
            let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
            let Err(error) = expand_inner(&input) else {
                panic!("an empty tagged enum must be rejected");
            };
            assert!(error.to_string().contains("no branch for its `oneOf`"));
        }

        let accepted = [
            quote::quote!(
                enum Never {}
            ),
            // A variant serde only reads is still a branch.
            quote::quote!(
                #[serde(tag = "kind")]
                enum Signal {
                    #[serde(skip)]
                    Start,
                    #[serde(skip_serializing)]
                    Stop,
                }
            ),
        ];
        for declaration in accepted {
            let input: DeriveInput = syn::parse2(declaration).expect("the case itself must parse");
            assert!(expand_inner(&input).is_ok());
        }
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

    /// Whether the expansion claims `kynos::schema::flatten::ClosedFlatten` for
    /// the input, read off the emitted tokens as [`claims_flatten`] reads its
    /// claim.
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

/// The parameter fields' shared grammar, `#[param]`, `#[header]` and
/// `#[cookie]`, driven through the `CookieParams` derive.
mod attribute {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::cookies::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "`value`, which fixes a header and is no cookie key",
                quote::quote!(
                    struct Session {
                        #[cookie(rename = "sid", value = "anonymous")]
                        id: String,
                    }
                ),
                "`value` is not part of the `#[cookie(...)]` grammar",
            ),
            case(
                "`rename` written twice, the second replacing the first",
                quote::quote!(
                    struct Theme {
                        #[cookie(rename = "theme")]
                        #[cookie(rename = "colour")]
                        theme: String,
                    }
                ),
                "`rename` is already set",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_attribute_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("attribute.rs", include_str!("attribute.rs"), ledger().len());
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
                "a `base` that is neither a prefix nor a `const` holding one",
                quote::quote!(
                    #[problem(status = 404, base = 42)]
                    struct StoreError;
                ),
                "`base` is a string literal, or the path to a `const`",
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

    /// The refusals of a key written twice, and of a field marker outside the
    /// field grammar; apart, so [`ledger`] stays one a reader can hold.
    fn keys_ledger() -> Vec<Case> {
        vec![
            case(
                "a title written twice, across two lists, when the second would replace the first",
                quote::quote!(
                    #[problem(status = 404, title = "User not found")]
                    #[problem(title = "Gone")]
                    struct StoreError;
                ),
                "`title` is already set",
            ),
            case(
                "a field marker outside the field grammar",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 404)]
                        NotFound {
                            #[problem(extention)]
                            id: u64,
                        },
                    }
                ),
                "is not part of a field's `#[problem(...)]` grammar",
            ),
            case(
                "a field marker written twice",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 404)]
                        NotFound {
                            #[problem(extension)]
                            #[problem(extension)]
                            id: u64,
                        },
                    }
                ),
                "`extension` is already set",
            ),
            case(
                "a field marker given a value, which a bare marker would drop",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 404)]
                        NotFound {
                            #[problem(extension = "user_id")]
                            id: u64,
                        },
                    }
                ),
                "`extension` is a bare marker and takes no value",
            ),
        ]
    }

    /// Each field marker is bare, given as `= value` or as a list; the ledger's
    /// one row witnesses `extension = ...` alone.
    #[test]
    fn a_field_marker_refuses_a_value_in_every_form() {
        each_case_is_refused(
            vec![
                case(
                    "an extension marker given a list",
                    quote::quote!(
                        struct StoreError {
                            #[problem(extension(user_id))]
                            id: u64,
                        }
                    ),
                    "`extension` is a bare marker and takes no value",
                ),
                case(
                    "a headers marker given a value",
                    quote::quote!(
                        struct StoreError {
                            #[problem(headers = "retry")]
                            retry: RetryAfter,
                        }
                    ),
                    "`headers` is a bare marker and takes no value",
                ),
                case(
                    "a headers marker given a list",
                    quote::quote!(
                        struct StoreError {
                            #[problem(headers(retry))]
                            retry: RetryAfter,
                        }
                    ),
                    "`headers` is a bare marker and takes no value",
                ),
            ],
            expand_inner,
        );
    }

    /// The grammar refuses a qualified key, which matches no key, and each key
    /// written twice, at every position; the ledger holds one row per site.
    #[test]
    fn every_position_refuses_qualified_and_repeated_keys() {
        each_case_is_refused(
            vec![
                case(
                    "a qualified key on the type",
                    quote::quote!(
                        #[problem(status = 404, kynos::title = "User not found")]
                        struct StoreError;
                    ),
                    "`kynos::title` is not part of the `#[problem(...)]` grammar",
                ),
                case(
                    "a qualified key on a variant",
                    quote::quote!(
                        enum StoreError {
                            #[problem(status = 404, a::b)]
                            NotFound,
                        }
                    ),
                    "`a::b` is not part of the `#[problem(...)]` grammar",
                ),
                case(
                    "a qualified field marker",
                    quote::quote!(
                        enum StoreError {
                            #[problem(status = 404)]
                            NotFound {
                                #[problem(kynos::extension)]
                                id: u64,
                            },
                        }
                    ),
                    "`kynos::extension` is not part of a field's `#[problem(...)]` grammar",
                ),
                case(
                    "a type written twice in one list",
                    quote::quote!(
                        #[problem(
                            status = 404,
                            type = "https://a.example/",
                            type = "https://b.example/"
                        )]
                        struct StoreError;
                    ),
                    "`type` is already set",
                ),
                case(
                    "a title written twice on a variant",
                    quote::quote!(
                        enum StoreError {
                            #[problem(status = 404, title = "a", title = "b")]
                            NotFound,
                        }
                    ),
                    "`title` is already set",
                ),
                case(
                    "a base written twice on the type",
                    quote::quote!(
                        #[problem(base = "https://a.example/")]
                        #[problem(base = "https://b.example/")]
                        enum StoreError {
                            #[problem(status = 404)]
                            NotFound,
                        }
                    ),
                    "`base` is already set",
                ),
                case(
                    "transparent written twice",
                    quote::quote!(
                        enum StoreError {
                            #[problem(transparent, transparent)]
                            Lookup(LookupError),
                        }
                    ),
                    "`transparent` is already set",
                ),
                case(
                    "the headers marker written twice",
                    quote::quote!(
                        enum StoreError {
                            #[problem(status = 429)]
                            Limited {
                                #[problem(headers, headers)]
                                retry: RetryAfter,
                            },
                        }
                    ),
                    "`headers` is already set",
                ),
            ],
            expand_inner,
        );
    }

    /// The refusals of `#[problem(headers)]`, apart for the reason
    /// [`transparent_ledger`] is.
    fn headers_ledger() -> Vec<Case> {
        vec![
            case(
                "`headers` on a variant, naming the group the field should hold",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 429, headers = RetryAfter)]
                        Throttled { retry: RetryAfter },
                    }
                ),
                "belongs on that field, whose type is the group",
            ),
            case(
                "a field marked both an extension member and a header group",
                quote::quote!(
                    enum StoreError {
                        #[problem(status = 429)]
                        Throttled {
                            #[problem(extension, headers)]
                            retry: RetryAfter,
                        },
                    }
                ),
                "not both",
            ),
            case(
                "a second header group on one failure",
                quote::quote!(
                    #[problem(status = 429)]
                    struct Throttled {
                        #[problem(headers)]
                        retry: RetryAfter,
                        #[problem(headers)]
                        limit: RateLimit,
                    }
                ),
                "already sends the header group in `retry`",
            ),
            case(
                "a header group on a transparent variant's field",
                quote::quote!(
                    enum StoreError {
                        #[problem(transparent)]
                        Lookup(#[problem(headers)] LookupError),
                    }
                ),
                "sends the headers of the error it holds",
            ),
        ]
    }

    /// A second function rather than more rows in the first, for the reason
    /// `security_scheme`'s `oauth2_ledger` gives. These are the refusals of
    /// `#[problem(transparent)]`.
    fn transparent_ledger() -> Vec<Case> {
        vec![
            case(
                "`transparent` on the type, when it hands over one variant",
                quote::quote!(
                    #[problem(transparent)]
                    struct StoreError(LookupError);
                ),
                "belongs on a variant of an enum",
            ),
            case(
                "a transparent variant that also declares a status",
                quote::quote!(
                    enum StoreError {
                        #[problem(transparent, status = 404)]
                        Lookup(LookupError),
                    }
                ),
                "answers as the error it holds",
            ),
            case(
                "a transparent variant holding more than one field",
                quote::quote!(
                    enum StoreError {
                        #[problem(transparent)]
                        Lookup(LookupError, u64),
                    }
                ),
                "holds exactly one field",
            ),
            case(
                "an extension on a transparent variant's field",
                quote::quote!(
                    enum StoreError {
                        #[problem(transparent)]
                        Lookup {
                            #[problem(extension)]
                            source: LookupError,
                        },
                    }
                ),
                "publishes the extension members of the error it holds",
            ),
            case(
                "a transparent variant naming the type's generic parameter",
                quote::quote!(
                    enum StoreError<'a, E> {
                        #[problem(transparent)]
                        Lookup(Wrapped<&'a E>),
                    }
                ),
                "cannot depend on the type's generic parameter `'a`",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(transparent_ledger(), expand_inner);
        each_case_is_refused(headers_ledger(), expand_inner);
        each_case_is_refused(keys_ledger(), expand_inner);
    }

    /// One diagnostic site refuses three keys, so the ledger's single row per
    /// site leaves `title` and `type` unwitnessed; each must fire alone.
    #[test]
    fn a_transparent_variant_refuses_each_key_it_would_contradict_alone() {
        each_case_is_refused(
            vec![
                case(
                    "a transparent variant that also declares a title",
                    quote::quote!(
                        enum StoreError {
                            #[problem(transparent, title = "User not found")]
                            Lookup(LookupError),
                        }
                    ),
                    "answers as the error it holds",
                ),
                case(
                    "a transparent variant that also declares a type",
                    quote::quote!(
                        enum StoreError {
                            #[problem(transparent, type = "https://errors.example.com/lookup")]
                            Lookup(LookupError),
                        }
                    ),
                    "answers as the error it holds",
                ),
            ],
            expand_inner,
        );
    }

    #[test]
    fn every_api_error_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "api_error.rs` and `api_error/",
            &[
                include_str!("api_error.rs"),
                include_str!("api_error/args.rs"),
                include_str!("api_error/emit.rs"),
                include_str!("api_error/outcomes.rs"),
            ]
            .concat(),
            ledger().len()
                + transparent_ledger().len()
                + headers_ledger().len()
                + keys_ledger().len(),
        );
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
        let mut rows = vec![
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
                "a struct variant, when a body is one described type",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 201)]
                        Created { id: u32, revision: u32 },
                    }
                ),
                "carries its response body",
            ),
            case(
                "a body at a status whose response carries no content",
                quote::quote!(
                    enum DeleteReply {
                        #[reply(status = 204)]
                        Deleted(u32),
                    }
                ),
                "a `204` response carries no content",
            ),
        ];
        rows.extend(member_ledger());
        rows
    }

    /// The rows for the `#[reply(...)]` members themselves: where each goes,
    /// how often, and what is outside the grammar.
    fn member_ledger() -> Vec<Case> {
        vec![
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
                "a description on the enum, which no response would carry",
                quote::quote!(
                    #[reply(description = "what creating answers")]
                    enum CreateReply {
                        #[reply(status = 200)]
                        Ok(u32),
                    }
                ),
                "a description belongs on each variant",
            ),
            case(
                "two descriptions on one variant, the second replacing the first",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 200, description = "created")]
                        #[reply(description = "made")]
                        Ok(u32),
                    }
                ),
                "already declares a description",
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
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    /// A qualified key is outside the grammar too, rather than read as no
    /// key at all.
    #[test]
    fn a_qualified_key_is_refused() {
        each_case_is_refused(
            vec![case(
                "a qualified `description`",
                quote::quote!(
                    enum CreateReply {
                        #[reply(status = 200, reply::description = "created")]
                        Ok(u32),
                    }
                ),
                "is not part of the `#[reply(...)]` grammar",
            )],
            expand_inner,
        );
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
            case(
                "an API key named outside the token grammar",
                quote::quote!(
                    #[security(api_key(in = "header", name = "X Api Key"))]
                    struct ApiKey;
                ),
                "contains ' '",
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
                "a device authorization URL on a flow other than the device grant",
                quote::quote!(
                    #[security(oauth2(authorization_code(
                        authorization_url = "https://auth.example.com/authorize",
                        token_url = "https://auth.example.com/token",
                        device_authorization_url = "https://auth.example.com/device"
                    )))]
                    struct Delegated;
                ),
                "applies only to the `device_authorization` flow",
            ),
            case(
                "a carrier setting that is not the one word it takes",
                quote::quote!(
                    #[security(bearer)]
                    #[security(carrier = automatic)]
                    struct Bearer;
                ),
                "takes `manual`",
            ),
            case(
                "a carrier set twice, which could only disagree with itself",
                quote::quote!(
                    #[security(bearer)]
                    #[security(carrier = optional)]
                    #[security(carrier = manual)]
                    struct Bearer;
                ),
                "`carrier` is already set",
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

    /// The refusals of an option outside its kind or flow, and of a key
    /// written twice; apart for the reason [`oauth2_ledger`] is.
    fn keys_ledger() -> Vec<Case> {
        vec![
            case(
                "a scheme option written twice, across two lists",
                quote::quote!(
                    #[security(bearer, description = "A token")]
                    #[security(description = "Another")]
                    struct Bearer;
                ),
                "`description` is already set",
            ),
            case(
                "an option its kind does not model, misspelled",
                quote::quote!(
                    #[security(bearer(formt = "JWT"))]
                    struct Bearer;
                ),
                "`formt` is not part of the `bearer(...)` grammar",
            ),
            case(
                "a bare kind given a value, which nothing would read",
                quote::quote!(
                    #[security(bearer = "JWT")]
                    struct Bearer;
                ),
                "`bearer` takes its options in a list",
            ),
            case(
                "a kind's option written twice",
                quote::quote!(
                    #[security(api_key(in = "header", name = "X-Api-Key", name = "X-Key"))]
                    struct ApiKey;
                ),
                "`name` is already set, and `api_key(...)` takes each option once",
            ),
            case(
                "a key no flow models",
                quote::quote!(
                    #[security(oauth2(client_credentials(
                        token_url = "https://auth.example.com/token",
                        token_uri = "https://auth.example.com/token",
                    )))]
                    struct Delegated;
                ),
                "`token_uri` is not part of the `client_credentials(...)` grammar",
            ),
            case(
                "a flow key written twice",
                quote::quote!(
                    #[security(oauth2(client_credentials(
                        token_url = "https://auth.example.com/a",
                        token_url = "https://auth.example.com/b",
                    )))]
                    struct Delegated;
                ),
                "`token_url` is already set, and `client_credentials(...)` takes each key once",
            ),
        ]
    }

    /// A kind with no options, given a value, is told to stand bare: pointing
    /// it at `kind(...)` would name a form its empty grammar refuses too.
    #[test]
    fn an_optionless_kind_given_a_value_is_told_to_stand_bare() {
        each_case_is_refused(
            vec![
                case(
                    "basic given a value",
                    quote::quote!(
                        #[security(basic = "x")]
                        struct Basic;
                    ),
                    "`basic` takes no options; write it bare, as `basic`, not with a value",
                ),
                case(
                    "mutual TLS given a value",
                    quote::quote!(
                        #[security(mutual_tls = "x")]
                        struct Mtls;
                    ),
                    "`mutual_tls` takes no options; write it bare, as `mutual_tls`, not with a \
                     value",
                ),
            ],
            expand_inner,
        );
    }

    /// Each kind takes only the options its row of the grammar lists, and each
    /// flow only the URLs the OAuth Flow Object applies to it; the ledger holds
    /// one row per site, so the rest of the table is swept here.
    #[test]
    fn each_kind_and_flow_refuses_an_option_outside_its_row() {
        let token = "https://auth.example.com/token";
        let authorize = "https://auth.example.com/authorize";
        each_case_is_refused(
            vec![
                case(
                    "a URL on a kind that has none",
                    quote::quote!(
                        #[security(bearer(url = "https://example.com"))]
                        struct Bearer;
                    ),
                    "`url` is not part of the `bearer(...)` grammar",
                ),
                case(
                    "an option on basic, which has none",
                    quote::quote!(
                        #[security(basic(format = "JWT"))]
                        struct Basic;
                    ),
                    "`format` is not part of the `basic(...)` grammar",
                ),
                case(
                    "an option on mutual TLS, which has none",
                    quote::quote!(
                        #[security(mutual_tls(name = "client"))]
                        struct Mtls;
                    ),
                    "`name` is not part of the `mutual_tls(...)` grammar",
                ),
                case(
                    "an API key's location on an HTTP scheme",
                    quote::quote!(
                        #[security(http(scheme = "Digest", in = "header"))]
                        struct Digest;
                    ),
                    "`in` is not part of the `http(...)` grammar",
                ),
                case(
                    "a format on an API key",
                    quote::quote!(
                        #[security(api_key(in = "header", name = "X-Api-Key", format = "JWT"))]
                        struct ApiKey;
                    ),
                    "`format` is not part of the `api_key(...)` grammar",
                ),
                case(
                    "a format on OpenID Connect",
                    quote::quote!(
                        #[security(openid_connect(url = "https://example.com", format = "JWT"))]
                        struct Oidc;
                    ),
                    "`format` is not part of the `openid_connect(...)` grammar",
                ),
                case(
                    "an authorization URL on the client credentials flow",
                    quote::quote!(
                        #[security(oauth2(client_credentials(
                            token_url = #token,
                            authorization_url = #authorize,
                        )))]
                        struct Delegated;
                    ),
                    "`authorization_url` is not part of the `client_credentials(...)` grammar",
                ),
                case(
                    "an authorization URL on the password flow",
                    quote::quote!(
                        #[security(oauth2(password(token_url = #token, authorization_url = #authorize)))]
                        struct Delegated;
                    ),
                    "`authorization_url` is not part of the `password(...)` grammar",
                ),
                case(
                    "a token URL on the implicit flow",
                    quote::quote!(
                        #[security(oauth2(implicit(authorization_url = #authorize, token_url = #token)))]
                        struct Delegated;
                    ),
                    "`token_url` is not part of the `implicit(...)` grammar",
                ),
            ],
            expand_inner,
        );
    }

    /// A qualified key matches no key at any level, so each level refuses it.
    #[test]
    fn every_level_refuses_a_qualified_key() {
        let token = "https://auth.example.com/token";
        each_case_is_refused(
            vec![
                case(
                    "a qualified option inside a kind",
                    quote::quote!(
                        #[security(bearer(kynos::format = "JWT"))]
                        struct Bearer;
                    ),
                    "`kynos::format` is not part of the `bearer(...)` grammar",
                ),
                case(
                    "a qualified option inside oauth2, which names no flow",
                    quote::quote!(
                        #[security(oauth2(a::client_credentials(token_url = #token)))]
                        struct Delegated;
                    ),
                    "`a::client_credentials` is not part of the `oauth2(...)` grammar",
                ),
                case(
                    "a qualified key inside a flow",
                    quote::quote!(
                        #[security(oauth2(client_credentials(a::token_url = #token)))]
                        struct Delegated;
                    ),
                    "`a::token_url` is not part of the `client_credentials(...)` grammar",
                ),
                case(
                    "a qualified scheme key",
                    quote::quote!(
                        #[security(bearer, kynos::name = "Token")]
                        struct Bearer;
                    ),
                    "`kynos::name` is not part of the `#[security(...)]` grammar",
                ),
            ],
            expand_inner,
        );
    }

    /// Every key is written once at every level, across lists too; the ledger
    /// holds one row per level, so the other keys are swept here.
    #[test]
    fn every_level_refuses_a_repeated_key() {
        let token = "https://auth.example.com/token";
        each_case_is_refused(
            vec![
                case(
                    "a flow's scopes written twice",
                    quote::quote!(
                        #[security(oauth2(client_credentials(
                            token_url = #token,
                            scopes("a"),
                            scopes("b"),
                        )))]
                        struct Delegated;
                    ),
                    "`scopes` is already set",
                ),
                case(
                    "a bearer format written twice",
                    quote::quote!(
                        #[security(bearer(format = "JWT", format = "opaque"))]
                        struct Bearer;
                    ),
                    "`format` is already set, and `bearer(...)` takes each option once",
                ),
                case(
                    "an OpenID Connect URL written twice",
                    quote::quote!(
                        #[security(openid_connect(
                            url = "https://a.example",
                            url = "https://b.example"
                        ))]
                        struct Oidc;
                    ),
                    "`url` is already set",
                ),
                case(
                    "each scheme key written twice",
                    quote::quote!(
                        #[security(bearer, name = "A", name = "B")]
                        struct Bearer;
                    ),
                    "`name` is already set, and `#[security(...)]` takes each key once",
                ),
                case(
                    "a credential written twice",
                    quote::quote!(
                        #[security(bearer, credential = A, credential = B)]
                        struct Bearer;
                    ),
                    "`credential` is already set",
                ),
                case(
                    "a scope type written twice",
                    quote::quote!(
                        #[security(bearer, scope = A, scope = B)]
                        struct Bearer;
                    ),
                    "`scope` is already set",
                ),
                case(
                    "a challenge written twice",
                    quote::quote!(
                        #[security(bearer, challenge = "Bearer")]
                        #[security(challenge = "Bearer realm=\"a\"")]
                        struct Bearer;
                    ),
                    "`challenge` is already set",
                ),
                case(
                    "the demanded scopes written twice",
                    quote::quote!(
                        #[security(bearer, scopes(A::Read), scopes(A::Write))]
                        struct Bearer;
                    ),
                    "`scopes` is already set",
                ),
            ],
            expand_inner,
        );
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

    /// The two OAuth 2.0 refusals a 3.1 build raises, word for word.
    ///
    /// The UI suite records an `--all-features` build, where neither fires, so
    /// this is the only place their wording is held. Both once carried the
    /// indentation of a lost line continuation as a run of spaces mid-sentence.
    #[cfg(not(feature = "openapi32"))]
    #[test]
    fn version_gated_oauth2_diagnostics_read_as_one_sentence() {
        let cases: [(syn::DeriveInput, &str); 2] = [
            (
                syn::parse_quote!(
                    #[security(oauth2(device_authorization(
                        device_authorization_url = "https://auth.example.com/device",
                        token_url = "https://auth.example.com/token"
                    )))]
                    struct Delegated;
                ),
                "the `device_authorization` flow was introduced in OpenAPI 3.2, and this build \
                 describes 3.1; enable the `openapi32` feature, or declare a flow 3.1 can express",
            ),
            (
                syn::parse_quote!(
                    #[security(oauth2(
                        client_credentials(token_url = "https://auth.example.com/token"),
                        metadata_url = "https://auth.example.com/meta",
                    ))]
                    struct Delegated;
                ),
                "`metadata_url` writes `oauth2MetadataUrl`, which OpenAPI 3.2 introduced, and \
                 this build describes 3.1; enable the `openapi32` feature, or drop it",
            ),
        ];

        for (input, expected) in cases {
            let error = expand_inner(&input).expect_err("a 3.1 build refuses it");
            assert_eq!(error.to_string(), expected);
        }
    }

    /// How many diagnostics this build cannot provoke.
    ///
    /// Three, under `openapi32`: the constructs they refuse are legal there.
    const UNREACHABLE_HERE: usize = if cfg!(feature = "openapi32") { 3 } else { 0 };

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(oauth2_ledger(), expand_inner);
        each_case_is_refused(keys_ledger(), expand_inner);
        each_case_is_refused(version_gated_ledger(), expand_inner);
    }

    #[test]
    fn every_security_scheme_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "security_scheme.rs` and `security_scheme/",
            &[
                include_str!("security_scheme.rs"),
                include_str!("security_scheme/args.rs"),
                include_str!("security_scheme/flow.rs"),
            ]
            .concat(),
            ledger().len()
                + oauth2_ledger().len()
                + keys_ledger().len()
                + version_gated_ledger().len()
                + UNREACHABLE_HERE,
        );
    }

    /// `deviceAuthorizationUrl` applies to the `deviceAuthorization` flow alone
    /// (OpenAPI 3.2's OAuth Flow Object), so the ledger's one row is repeated
    /// on every other flow, each otherwise complete; in a 3.1 build too, where
    /// the key used to be dropped rather than refused.
    #[test]
    fn a_device_authorization_url_is_refused_on_every_other_flow() {
        let device = "https://auth.example.com/device";
        each_case_is_refused(
            vec![
                case(
                    "a device authorization URL on the implicit flow",
                    quote::quote!(
                        #[security(oauth2(implicit(
                            authorization_url = "https://auth.example.com/authorize",
                            device_authorization_url = #device
                        )))]
                        struct Delegated;
                    ),
                    "applies only to the `device_authorization` flow",
                ),
                case(
                    "a device authorization URL on the password flow",
                    quote::quote!(
                        #[security(oauth2(password(
                            token_url = "https://auth.example.com/token",
                            device_authorization_url = #device
                        )))]
                        struct Delegated;
                    ),
                    "applies only to the `device_authorization` flow",
                ),
                case(
                    "a device authorization URL on the client credentials flow",
                    quote::quote!(
                        #[security(oauth2(client_credentials(
                            token_url = "https://auth.example.com/token",
                            device_authorization_url = #device
                        )))]
                        struct Delegated;
                    ),
                    "applies only to the `device_authorization` flow",
                ),
                case(
                    "a device authorization URL on the authorization code flow",
                    quote::quote!(
                        #[security(oauth2(authorization_code(
                            authorization_url = "https://auth.example.com/authorize",
                            token_url = "https://auth.example.com/token",
                            device_authorization_url = #device
                        )))]
                        struct Delegated;
                    ),
                    "applies only to the `device_authorization` flow",
                ),
            ],
            expand_inner,
        );
    }

    /// The device grant, the one flow the key applies to, still carries it.
    #[cfg(feature = "openapi32")]
    #[test]
    fn a_device_authorization_url_reaches_the_device_flow() {
        let input: syn::DeriveInput = syn::parse_quote!(
            #[security(oauth2(device_authorization(
                device_authorization_url = "https://auth.example.com/device",
                token_url = "https://auth.example.com/token"
            )))]
            struct Delegated;
        );

        let expanded = expand_inner(&input)
            .expect("a well-formed device flow")
            .to_string();

        assert!(
            expanded
                .contains("with_device_authorization_url (\"https://auth.example.com/device\")"),
            "the device flow drops its URL: {expanded}"
        );
    }

    /// An API key's name is held to the token grammar exactly where the wire
    /// holds it there: a header field name (RFC 9110 §5.1) and a cookie name
    /// (RFC 6265 §4.1.1) are tokens, while a query parameter name is any
    /// string its percent-encoding can carry.
    #[test]
    fn an_api_key_name_is_a_token_only_in_a_header_or_a_cookie() {
        for (location, token_only) in [("header", true), ("cookie", true), ("query", false)] {
            let input: syn::DeriveInput = syn::parse_quote!(
                #[security(api_key(in = #location, name = "X Api Key"))]
                struct ApiKey;
            );
            assert_eq!(expand_inner(&input).is_err(), token_only, "{location}");
        }
    }

    /// `carrier = optional` wraps the kind's own reader rather than replacing
    /// it, so a malformed credential is still the reader's `Err` and only
    /// absence becomes a value; `carrier = manual` writes no carrier at all.
    #[test]
    fn an_optional_carrier_wraps_the_kind_reader_in_some() {
        let carrier = |mode: Option<&str>| {
            let mode = mode.map(|mode| syn::Ident::new(mode, proc_macro2::Span::call_site()));
            let setting = mode.map(|mode| quote::quote!(#[security(carrier = #mode)]));
            let input: syn::DeriveInput = syn::parse_quote!(
                #[security(bearer)]
                #setting
                struct Bearer;
            );
            let expanded = expand_inner(&input).expect("a well-formed bearer scheme");
            let file: syn::File = syn::parse2(expanded).expect("the expansion parses");
            file.items.into_iter().find_map(|item| match item {
                syn::Item::Impl(implementation)
                    if implementation.trait_.as_ref().is_some_and(|(_, path, _)| {
                        path.segments
                            .last()
                            .is_some_and(|last| last.ident == "Carries")
                    }) =>
                {
                    Some(quote::quote!(#implementation).to_string())
                }
                _ => None,
            })
        };

        let derived = carrier(None).expect("a derived carrier");
        assert!(
            derived.contains("type Presented = :: kynos :: security :: carrier :: BearerToken ;"),
            "{derived}"
        );
        assert!(!derived.contains("Option :: Some"), "{derived}");

        let optional = carrier(Some("optional")).expect("an optional carrier");
        assert!(
            optional.contains(
                "type Presented = :: core :: option :: Option < :: kynos :: security :: carrier :: \
                 BearerToken > ;"
            ),
            "{optional}"
        );
        assert!(
            optional.contains(
                ":: kynos :: security :: carrier :: bearer (parts) . map (:: core :: option :: \
                 Option :: Some)"
            ),
            "{optional}"
        );

        assert_eq!(carrier(Some("manual")), None);
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
            case(
                "a misspelled `skip`, which once provided the field anyway",
                quote::quote!(
                    struct App {
                        pool: Pool,
                        #[provide(skp)]
                        started_at: Instant,
                    }
                ),
                "is not part of the `#[provide(...)]` grammar",
            ),
            case(
                "`skip` written twice",
                quote::quote!(
                    struct App {
                        pool: Pool,
                        #[provide(skip)]
                        #[provide(skip)]
                        started_at: Instant,
                    }
                ),
                "`skip` is already set",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    /// A key outside the grammar is refused whatever its shape, a qualified
    /// path included, rather than read as no key at all.
    #[test]
    fn an_unknown_key_is_refused_in_every_shape() {
        each_case_is_refused(
            ["skip_it = true", "skip_it(always)", "di::skip"]
                .into_iter()
                .map(|written| {
                    let written: proc_macro2::TokenStream =
                        written.parse().expect("the key itself must parse");
                    case(
                        "a key the grammar does not hold",
                        quote::quote!(
                            struct App {
                                #[provide(#written)]
                                started_at: Instant,
                            }
                        ),
                        "is not part of the `#[provide(...)]` grammar",
                    )
                })
                .collect(),
            expand_inner,
        );
    }

    /// `skip` is bare. Given a value, the list fails to parse, and that
    /// failure reaches the user instead of being discarded with the field
    /// skipped anyway.
    #[test]
    fn a_skip_given_a_value_is_refused() {
        each_case_is_refused(
            ["skip = true", "skip(always)"]
                .into_iter()
                .map(|written| {
                    let written: proc_macro2::TokenStream =
                        written.parse().expect("the key itself must parse");
                    case(
                        "`skip` given a value",
                        quote::quote!(
                            struct App {
                                #[provide(#written)]
                                started_at: Instant,
                            }
                        ),
                        "",
                    )
                })
                .collect(),
            expand_inner,
        );
    }

    #[test]
    fn every_provider_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("provider.rs", include_str!("provider.rs"), ledger().len());
    }
}

mod headers {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::headers::expand_inner;
    use syn::DeriveInput;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a header the framework already negotiates",
                quote::quote!(
                    struct Negotiation {
                        accept: String,
                    }
                ),
                "must not be declared as a header parameter",
            ),
            case(
                "a `()` field naming no value to stand for",
                quote::quote!(
                    struct Sniffing {
                        #[header(rename = "X-Content-Type-Options")]
                        nosniff: (),
                    }
                ),
                "needs the one it always carries",
            ),
            case(
                "a fixed value on a field that carries one of its own",
                quote::quote!(
                    struct Sniffing {
                        #[header(rename = "X-Content-Type-Options", value = "nosniff")]
                        nosniff: String,
                    }
                ),
                "so its field is `()`",
            ),
            case(
                "a fixed value no field value can be",
                quote::quote!(
                    struct Sniffing {
                        #[header(rename = "X-Content-Type-Options", value = "nosniff\n")]
                        nosniff: (),
                    }
                ),
                "contains '\\n'",
            ),
            case(
                "a bound on a fixed value",
                quote::quote!(
                    struct Sniffing {
                        #[schema(max_length = 4)]
                        #[header(rename = "X-Content-Type-Options", value = "nosniff")]
                        nosniff: (),
                    }
                ),
                "has nothing left to bound",
            ),
        ]
    }

    /// A fixed value is RFC 9110 §5.5's `field-value` a decoder can read as
    /// text: printable ASCII, with inner but no outer whitespace, which a
    /// recipient would strip before the comparison. An optional field takes one
    /// as a required field does.
    #[test]
    fn a_fixed_value_is_a_field_value_read_as_text() {
        for (value, refusal) in [
            ("max-age=0; includeSubDomains", None),
            ("a\tb", None),
            ("", None),
            ("café", Some("contains 'é'")),
            ("a\u{7f}", Some("contains '\\u{7f}'")),
            (" nosniff", Some("starts or ends with whitespace")),
            ("nosniff\t", Some("starts or ends with whitespace")),
        ] {
            let input: DeriveInput = syn::parse_quote!(
                struct Sniffing {
                    #[header(rename = "X-Content-Type-Options", value = #value)]
                    nosniff: Option<()>,
                }
            );
            match (expand_inner(&input), refusal) {
                (Ok(_), None) => {}
                (Err(error), Some(expected)) => {
                    let message = error.to_string();
                    assert!(message.contains(expected), "{value:?}: {message}");
                }
                (Ok(_), Some(expected)) => panic!("{value:?} was accepted; expected {expected}"),
                (Err(error), None) => panic!("{value:?} was refused: {error}"),
            }
        }
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    /// A field is checked under the name the header carries, not its
    /// identifier: `content_type` is no header until `rename_all` makes it
    /// `content-type`.
    ///
    /// Beside the ledger rather than in it: the ledger's row proves the site
    /// fires, and this proves it reads the final name.
    #[test]
    fn a_reserved_name_reached_through_rename_all_is_refused() {
        let unrenamed: DeriveInput = syn::parse_quote!(
            struct Body {
                content_type: String,
            }
        );
        expand_inner(&unrenamed).expect("`content_type` is not a reserved header name");
        each_case_is_refused(
            vec![case(
                "`content_type` under `rename_all = \"kebab-case\"`",
                quote::quote!(
                    #[serde(rename_all = "kebab-case")]
                    struct Body {
                        content_type: String,
                    }
                ),
                "`content-type` must not be declared as a header parameter",
            )],
            expand_inner,
        );
    }

    /// A field name is an RFC 9110 §5.1 token, so a name outside that grammar
    /// is refused, whichever source gave it: `HeaderName::from_static` would
    /// panic on it in `encode`, and no request could ever carry it to `decode`.
    #[test]
    fn a_name_outside_the_token_grammar_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "a space, from the Kynos `rename`",
                    quote::quote!(
                        struct Tracing {
                            #[header(rename = "X Id")]
                            id: String,
                        }
                    ),
                    "contains ' '",
                ),
                case(
                    "a delimiter, from serde's `rename`",
                    quote::quote!(
                        struct Tracing {
                            #[serde(rename = "x:id")]
                            id: String,
                        }
                    ),
                    "contains ':'",
                ),
                case(
                    "a character outside ASCII",
                    quote::quote!(
                        struct Tracing {
                            #[header(rename = "x-café")]
                            id: String,
                        }
                    ),
                    "contains 'é'",
                ),
                case(
                    "no character at all",
                    quote::quote!(
                        struct Tracing {
                            #[header(rename = "")]
                            id: String,
                        }
                    ),
                    "is empty",
                ),
            ],
            expand_inner,
        );
    }

    /// Every character the token grammar allows, in either case, is accepted.
    #[test]
    fn a_name_of_every_token_character_is_accepted() {
        let input: DeriveInput = syn::parse_quote!(
            struct Tracing {
                #[header(rename = "Az09!#$%&'*+-.^_`|~")]
                id: String,
            }
        );
        if let Err(error) = expand_inner(&input) {
            panic!("a token was refused as a header name: {error}");
        }
    }

    /// The `#[header]` grammar is closed: a misspelled `value` is named as
    /// the mistake it is, and a second `value` does not replace the first.
    /// Both are `common.rs`'s diagnostics, counted there; these rows hold the
    /// header derive to them.
    #[test]
    fn the_header_grammar_refuses_unknown_and_repeated_keys() {
        each_case_is_refused(
            vec![
                case(
                    "a misspelled `value`",
                    quote::quote!(
                        struct Hardened {
                            #[header(rename = "X-Content-Type-Options", vaule = "nosniff")]
                            nosniff: (),
                        }
                    ),
                    "`vaule` is not part of the `#[header(...)]` grammar",
                ),
                case(
                    "`value` written twice",
                    quote::quote!(
                        struct Hardened {
                            #[header(rename = "X-Content-Type-Options", value = "nosniff")]
                            #[header(value = "sniff")]
                            nosniff: (),
                        }
                    ),
                    "`value` is already set",
                ),
            ],
            expand_inner,
        );
    }

    /// Field names are case-insensitive (RFC 9110 §5.1), so two spellings of
    /// one name read the same header and are refused as one name declared twice.
    #[test]
    fn names_differing_only_in_case_are_duplicates() {
        each_case_is_refused(
            vec![case(
                "`X-Id` beside `x-id`",
                quote::quote!(
                    struct Tracing {
                        #[header(rename = "X-Id")]
                        upper: String,
                        #[header(rename = "x-id")]
                        lower: String,
                    }
                ),
                "two fields declare the header",
            )],
            expand_inner,
        );
    }

    #[test]
    fn every_headers_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("headers.rs", include_str!("headers.rs"), ledger().len());
    }

    /// The model says which headers are refused and the derive only what to
    /// reach for instead, so a header the model adds needs advice here before
    /// the refusal can name it.
    #[test]
    fn every_header_the_model_ignores_has_advice() {
        for name in kynos_openapi::model::parameter::header::IGNORED_HEADER_PARAMETERS {
            assert!(
                crate::derive::headers::advice(name).is_some(),
                "`{name}` is refused with no advice"
            );
        }
    }
}

mod cookies {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::cookies::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![case(
            "`form`, which would tell a client to percent-encode a value read as sent",
            quote::quote!(
                struct Session {
                    #[cookie(style = "form")]
                    id: String,
                }
            ),
            "the one style it can state",
        )]
    }

    /// The refusal that fires only where the document model has no `cookie`
    /// style; under `openapi32` it cannot be provoked and has no row.
    #[cfg(not(feature = "openapi32"))]
    fn version_gated_ledger() -> Vec<Case> {
        vec![case(
            "the `cookie` style, which only 3.2 defines",
            quote::quote!(
                struct Session {
                    #[cookie(style = "cookie")]
                    id: String,
                }
            ),
            "enable the `openapi32` feature",
        )]
    }

    #[cfg(feature = "openapi32")]
    fn version_gated_ledger() -> Vec<Case> {
        Vec::new()
    }

    /// How many diagnostics this build cannot provoke.
    const UNREACHABLE_HERE: usize = if cfg!(feature = "openapi32") { 1 } else { 0 };

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(version_gated_ledger(), expand_inner);
    }

    #[test]
    fn every_cookies_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "cookies.rs",
            include_str!("cookies.rs"),
            ledger().len() + version_gated_ledger().len() + UNREACHABLE_HERE,
        );
    }

    /// `style` is the `#[cookie]` grammar's alone: the other parameter
    /// locations read values through their own decoders.
    #[test]
    fn style_is_no_header_key() {
        let input: syn::DeriveInput = syn::parse_quote!(
            struct Traced {
                #[header(style = "cookie")]
                id: String,
            }
        );
        let Err(error) = crate::derive::headers::expand_inner(&input) else {
            panic!("`#[header(style)]` was accepted");
        };
        assert!(
            error
                .to_string()
                .contains("not part of the `#[header(...)]` grammar"),
            "{error}"
        );
    }

    /// A cookie name is an RFC 6265 §4.1.1 token: `=` would end it and `;`
    /// would end the pair, so a cookie declared under either could never match.
    #[test]
    fn a_name_outside_the_token_grammar_is_refused() {
        each_case_is_refused(
            vec![
                case(
                    "the name-value separator",
                    quote::quote!(
                        struct Session {
                            #[cookie(rename = "a=b")]
                            id: String,
                        }
                    ),
                    "contains '='",
                ),
                case(
                    "the pair separator, from serde's `rename`",
                    quote::quote!(
                        struct Session {
                            #[serde(rename = "a;b")]
                            id: String,
                        }
                    ),
                    "contains ';'",
                ),
                case(
                    "no character at all",
                    quote::quote!(
                        struct Session {
                            #[cookie(rename = "")]
                            id: String,
                        }
                    ),
                    "is empty",
                ),
            ],
            expand_inner,
        );
    }

    /// Cookie names are compared exactly, as `http::cookie::value_of` matches
    /// them, so names differing only in case are two cookies and both stand.
    #[test]
    fn names_differing_only_in_case_are_distinct() {
        let input: syn::DeriveInput = syn::parse_quote!(
            struct Session {
                #[cookie(rename = "SID")]
                upper: String,
                #[cookie(rename = "sid")]
                lower: String,
            }
        );
        if let Err(error) = expand_inner(&input) {
            panic!("two cookie names differing in case were refused: {error}");
        }
    }
}

mod path_params {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::path_params::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![case(
            "an optional field, when a captured variable is always present",
            quote::quote!(
                struct ReportPath {
                    name: Option<String>,
                }
            ),
            "a path parameter is always present",
        )]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_path_params_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "path_params.rs",
            include_str!("path_params.rs"),
            ledger().len(),
        );
    }
}

mod multipart {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::multipart::expand_inner;

    /// A field serde only reads is still refused: a part has one name in both
    /// directions, whichever side serde uses. A container `rename_all` whose
    /// sides differ is refused beside it, since it names every part twice. The
    /// serde keys the description honours and the decoder could not follow
    /// are refused after them.
    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a split `rename` whose sides differ on a field serde only reads",
                quote::quote!(
                    struct Upload {
                        #[serde(
                            skip_serializing,
                            default,
                            rename(serialize = "a", deserialize = "b")
                        )]
                        caption: String,
                    }
                ),
                "gives this field two part names",
            ),
            case(
                "a split container `rename_all` whose sides differ",
                quote::quote!(
                    #[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
                    struct Upload {
                        file_name: String,
                    }
                ),
                "gives every field two part names",
            ),
            case(
                "a container `default`, which a form read part by part never builds",
                quote::quote!(
                    #[serde(default)]
                    struct Upload {
                        caption: String,
                    }
                ),
                "fills a field left out from the form's own `Default`",
            ),
            case(
                "a container `tag`, a required property no part carries",
                quote::quote!(
                    #[serde(tag = "kind")]
                    struct Upload {
                        caption: String,
                    }
                ),
                "describes a required property named by the tag",
            ),
            case(
                "a container `transparent`, described as its one field's schema",
                quote::quote!(
                    #[serde(transparent)]
                    struct Upload {
                        caption: String,
                    }
                ),
                "describes the form as its one field's schema",
            ),
            case(
                "an `alias`, a second name the part is never read under",
                quote::quote!(
                    struct Upload {
                        #[serde(alias = "title")]
                        caption: String,
                    }
                ),
                "gives this field a second part name",
            ),
            case(
                "a `skip`, which leaves out a field the form still carries",
                quote::quote!(
                    struct Upload {
                        #[serde(skip)]
                        caption: String,
                    }
                ),
                "leaves this field out of what serde",
            ),
            case(
                "a `flatten`, which describes the field as its members",
                quote::quote!(
                    struct Upload {
                        #[serde(flatten)]
                        caption: String,
                    }
                ),
                "is described as its own members",
            ),
            case(
                "a `Vec` field without `default`, required yet carried by no part when empty",
                quote::quote!(
                    struct Upload {
                        images: Vec<FilePart>,
                    }
                ),
                "is carried by no part when it is empty",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    /// Every one of serde's `skip` keys leaves a field out of one direction at
    /// least, while a form reads and writes each field as its parts.
    #[test]
    fn every_skip_is_refused() {
        let skips = [
            quote::quote!(skip),
            quote::quote!(skip_serializing),
            quote::quote!(skip_deserializing),
            quote::quote!(skip_serializing_if = "Option::is_none"),
        ];
        let ledger = skips
            .into_iter()
            .map(|skip| {
                case(
                    "a `skip` key",
                    quote::quote!(
                        struct Upload {
                            #[serde(#skip)]
                            caption: Option<String>,
                        }
                    ),
                    "leaves this field out of what serde",
                )
            })
            .collect();
        each_case_is_refused(ledger, expand_inner);
    }

    /// A field-level `default`, bare or naming a function, is what the
    /// description reads as optional, and so is what the decoder fills; a
    /// `Vec` under it is the one form an empty list is described by.
    #[test]
    fn a_defaulted_field_is_accepted() {
        let input: syn::DeriveInput = syn::parse2(quote::quote!(
            struct Upload {
                #[serde(default)]
                caption: String,
                #[serde(default = "untitled")]
                title: String,
                #[serde(default)]
                images: Vec<FilePart>,
            }
        ))
        .expect("the case itself must parse");
        if let Err(error) = expand_inner(&input) {
            panic!("a defaulted field was refused: {error}");
        }
    }

    /// A split `rename` or `rename_all` whose sides agree gives the part one
    /// name, so it is accepted, as the single form is.
    #[test]
    fn a_rename_giving_one_part_name_is_accepted() {
        for declaration in [
            quote::quote!(
                #[serde(rename_all(serialize = "camelCase", deserialize = "camelCase"))]
                struct Upload {
                    file_name: String,
                }
            ),
            quote::quote!(
                struct Upload {
                    #[serde(rename(serialize = "a", deserialize = "a"))]
                    caption: String,
                }
            ),
            quote::quote!(
                struct Upload {
                    #[serde(rename = "a")]
                    caption: String,
                }
            ),
        ] {
            let input: syn::DeriveInput =
                syn::parse2(declaration).expect("the case itself must parse");
            if let Err(error) = expand_inner(&input) {
                panic!("a field with one part name was refused: {error}");
            }
        }
    }

    #[test]
    fn every_multipart_diagnostic_has_a_case() {
        every_diagnostic_has_a_case("multipart.rs", include_str!("multipart.rs"), ledger().len());
    }
}

mod tag {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::tag::expand_inner;

    /// The diagnostics every build can provoke.
    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a misspelled `description`, which once dropped the description",
                quote::quote!(
                    #[tag(descripton = "Managing user accounts")]
                    struct Users;
                ),
                "`descripton` is not part of the `#[tag(...)]` grammar",
            ),
            case(
                "`name` written twice, the second replacing the first",
                quote::quote!(
                    #[tag(name = "users")]
                    #[tag(name = "people")]
                    struct Users;
                ),
                "`name` is already set",
            ),
        ]
    }

    /// A key outside the grammar is refused whatever its shape, a qualified
    /// path included, rather than read as no key at all.
    #[test]
    fn an_unknown_key_is_refused_in_every_shape() {
        each_case_is_refused(
            [
                "nmae = \"users\"",
                "nmae(users)",
                "nmae",
                "tag::name = \"users\"",
            ]
            .into_iter()
            .map(|written| {
                let written: proc_macro2::TokenStream =
                    written.parse().expect("the key itself must parse");
                case(
                    "a key the grammar does not hold",
                    quote::quote!(
                        #[tag(#written)]
                        struct Users;
                    ),
                    "is not part of the `#[tag(...)]` grammar",
                )
            })
            .collect(),
            expand_inner,
        );
    }

    /// The 3.2-only members, which a 3.1 build refuses.
    ///
    /// Empty under `openapi32`, where all three are legal — the same shape
    /// [`super::security_scheme`] uses, and for the same reason: a diagnostic
    /// that only one build can provoke still has to be counted in both.
    #[cfg(not(feature = "openapi32"))]
    fn version_gated_ledger() -> Vec<Case> {
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
        each_case_is_refused(ledger(), expand_inner);
        each_case_is_refused(version_gated_ledger(), expand_inner);
    }

    #[test]
    fn every_tag_diagnostic_has_a_case() {
        // The three members are refused from one `syn::Error::new`, so the
        // gated ledger's three cases meet one site. Counting the *site* is the
        // point: a fourth 3.2 member added without a case fails here.
        let covered =
            ledger().len() + usize::from(!version_gated_ledger().is_empty()) + UNREACHABLE_HERE;
        every_diagnostic_has_a_case("tag.rs", include_str!("tag.rs"), covered);
    }
}

mod param_value {
    use super::{Case, case, each_case_is_refused, every_diagnostic_has_a_case};
    use crate::derive::param_value::expand_inner;

    fn ledger() -> Vec<Case> {
        vec![
            case(
                "a struct, which has no variant names",
                quote::quote!(
                    struct UserId(u64);
                ),
                "derived for a field-less enum",
            ),
            case(
                "a variant carrying data",
                quote::quote!(
                    enum Phase {
                        Checking,
                        Failed(String),
                    }
                ),
                "a variant declaring fields",
            ),
            case(
                "an enum with no variant",
                quote::quote!(
                    enum Never {}
                ),
                "no variant",
            ),
            case(
                "an internally tagged enum",
                quote::quote!(
                    #[serde(tag = "kind")]
                    enum Phase {
                        Checking,
                    }
                ),
                "Drop `tag`",
            ),
            case(
                "a variant serde never writes",
                quote::quote!(
                    enum Phase {
                        Checking,
                        #[serde(skip_serializing)]
                        Legacy,
                    }
                ),
                "`skip_serializing` leaves this variant",
            ),
        ]
    }

    #[test]
    fn each_case_raises_the_diagnostic_it_names() {
        each_case_is_refused(ledger(), expand_inner);
    }

    #[test]
    fn every_param_value_diagnostic_has_a_case() {
        every_diagnostic_has_a_case(
            "param_value.rs",
            include_str!("param_value.rs"),
            ledger().len(),
        );
    }

    /// The names come from the `Schema` derive's rules, so its refusals guard
    /// them here too: a split `rename` would let `Display` write a name
    /// `FromStr` never reads. A variant skipped both ways is refused before
    /// those rules, which leave it out of the schema, could drop it silently.
    #[test]
    fn the_schema_refusals_guard_the_names() {
        each_case_is_refused(
            vec![
                case(
                    "a split rename",
                    quote::quote!(
                        enum Phase {
                            #[serde(rename(serialize = "checking", deserialize = "check"))]
                            Checking,
                        }
                    ),
                    "reads it as `check`",
                ),
                case(
                    "a variant skipped both ways",
                    quote::quote!(
                        enum Phase {
                            Checking,
                            #[serde(skip)]
                            Legacy,
                        }
                    ),
                    "`skip` leaves this variant",
                ),
            ],
            expand_inner,
        );
    }
}
