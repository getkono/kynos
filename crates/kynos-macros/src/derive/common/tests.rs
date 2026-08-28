use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields, FieldsNamed};

use super::{named_fields, reject_duplicate_names, unit_struct, wire_name};

/// The single named field of a struct wrapped around `declaration`.
///
/// `Field` has no `Parse`, and a field only means anything inside the item
/// that holds it, so the wrapper is the shortest honest way to build one.
fn only_field(declaration: &TokenStream2) -> syn::Field {
    named(quote!(struct Holder { #declaration }))
        .named
        .into_iter()
        .next()
        .expect("one field")
}

fn item(declaration: TokenStream2) -> DeriveInput {
    syn::parse2(declaration).expect("an item")
}

fn named(declaration: TokenStream2) -> FieldsNamed {
    match item(declaration).data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => fields,
        _ => panic!("a struct with named fields"),
    }
}

/// Which of the three sources a wire name comes from, over every
/// combination of the two that can be absent.
///
/// A sweep rather than three examples: the rule is a precedence, and a
/// precedence is only wrong when two sources are present at once. Reading
/// serde's own `rename` is what stops a type describing one field name
/// while serializing another, so which one wins when both are set is the
/// whole point.
#[test]
fn a_wire_name_prefers_the_kynos_rename_then_serdes_then_the_identifier() {
    for kynos in [None, Some("from_kynos")] {
        for serde in [None, Some("from_serde")] {
            let kynos_attribute = kynos.map(|name| quote!(#[param(rename = #name)]));
            let serde_attribute = serde.map(|name| quote!(#[serde(rename = #name)]));
            let field = only_field(&quote! {
                #kynos_attribute
                #serde_attribute
                user_id: u64
            });

            let expected = kynos.or(serde).unwrap_or("user_id");
            assert_eq!(
                wire_name(&field, "param").expect("a wire name"),
                expected,
                "kynos: {kynos:?}, serde: {serde:?}"
            );
        }
    }
}

/// Each shape of value `skip_value` has to step over, with a `rename`
/// behind it.
///
/// The whole reason `skip_value` exists is that consuming the rest of the
/// input would swallow every later item, so an attribute would silently
/// lose everything after its first unrecognized key. That defect is
/// invisible from the outside -- the name simply falls back to the
/// identifier -- which is why the `rename` sits *after* the key being
/// skipped in every row.
#[test]
fn an_unrecognized_key_does_not_swallow_the_keys_after_it() {
    for (shape, skipped) in [
        ("a named value", quote!(unknown = 1)),
        ("a parenthesized group", quote!(unknown(a, b))),
        ("a bare path", quote!(unknown)),
    ] {
        let field = only_field(&quote! {
            #[param(#skipped, rename = "chosen")]
            user_id: u64
        });

        assert_eq!(
            wire_name(&field, "param").expect("a wire name"),
            "chosen",
            "{shape} must be stepped over, not consumed"
        );
    }
}

/// An attribute this derive does not model is not a mistake, so a key it
/// does not know is skipped rather than refused.
#[test]
fn an_unrecognized_key_alone_is_not_an_error() {
    let field = only_field(&quote! {
        #[param(unknown = 1)]
        user_id: u64
    });

    assert_eq!(wire_name(&field, "param").expect("a wire name"), "user_id");
}

/// A Kynos attribute belonging to another derive is not this one's to read.
#[test]
fn only_the_named_attribute_is_consulted() {
    let field = only_field(&quote! {
        #[header(rename = "X-Other")]
        user_id: u64
    });

    assert_eq!(wire_name(&field, "param").expect("a wire name"), "user_id");
}

#[test]
fn named_fields_accepts_a_struct_with_named_fields() {
    let input = item(quote!(
        struct Query {
            page: u32,
        }
    ));
    assert!(named_fields(&input, "QueryParams").is_ok());
}

#[test]
fn a_unit_struct_is_accepted_however_it_is_spelled() {
    for declaration in [
        quote!(
            struct Users;
        ),
        quote!(
            struct Users {}
        ),
    ] {
        let input = item(declaration);
        assert!(unit_struct(&input, "Tag", "names a group of operations").is_ok());
    }
}

#[test]
fn distinct_names_are_not_duplicates() {
    let fields = named(quote!(
        struct Query {
            page: u32,
            size: u32,
        }
    ));
    let names = ["page".to_owned(), "size".to_owned()];
    assert!(reject_duplicate_names(&fields, &names, "parameter").is_ok());
}

/// A duplicate names the field that claimed the wire name first, because a
/// diagnostic pointing at only the second says which field to change
/// without saying what it collides with.
#[test]
fn a_duplicate_names_the_field_that_claimed_it_first() {
    let fields = named(quote!(
        struct Query {
            page: u32,
            offset: u32,
        }
    ));
    let names = ["cursor".to_owned(), "cursor".to_owned()];

    let error = reject_duplicate_names(&fields, &names, "parameter")
        .expect_err("two fields on one wire name must be refused");
    let reported = error.to_string();

    assert!(reported.contains("two fields declare the parameter `cursor`"));
    assert!(reported.contains("the first is `page`"));
}

/// One row per diagnostic site in this module.
fn cases() -> Vec<(&'static str, syn::Result<()>, &'static str)> {
    fn shape(input: &DeriveInput) -> syn::Result<()> {
        named_fields(input, "QueryParams").map(|_| ())
    }
    fn unit(input: &DeriveInput) -> syn::Result<()> {
        unit_struct(input, "Tag", "names a group of operations")
    }

    let duplicate = named(quote!(
        struct Query {
            page: u32,
            offset: u32,
        }
    ));

    vec![
        (
            "named fields asked of a tuple struct",
            shape(&item(quote!(
                struct Query(u32);
            ))),
            "needs a struct with named fields",
        ),
        (
            "named fields asked of an enum",
            shape(&item(quote!(
                enum Query {
                    A,
                }
            ))),
            "which an enum is not",
        ),
        (
            "named fields asked of a union",
            shape(&item(quote!(
                union Query {
                    a: u32,
                }
            ))),
            "cannot describe a union",
        ),
        (
            "a unit struct asked of a struct with fields",
            unit(&item(quote!(
                struct Users {
                    name: String,
                }
            ))),
            "carries no fields",
        ),
        (
            "a unit struct asked of an enum",
            unit(&item(quote!(
                enum Users {
                    A,
                }
            ))),
            "must be a unit struct",
        ),
        (
            "a unit struct asked of a union",
            unit(&item(quote!(
                union Users {
                    a: u32,
                }
            ))),
            "must be a unit struct",
        ),
        (
            "serde's split rename, which gives one field two wire names",
            wire_name(
                &only_field(&quote! {
                    #[serde(rename(serialize = "a", deserialize = "b"))]
                    user_id: u64
                }),
                "param",
            )
            .map(|_| ()),
            "two wire names",
        ),
        (
            "two fields on one wire name",
            reject_duplicate_names(
                &duplicate,
                &["cursor".to_owned(), "cursor".to_owned()],
                "parameter",
            ),
            "two fields declare the",
        ),
    ]
}

#[test]
fn each_case_raises_the_diagnostic_it_names() {
    for (description, outcome, expected) in cases() {
        let Err(error) = outcome else {
            panic!("{description} must be rejected");
        };
        let reported = error.to_string();
        assert!(
            reported.contains(expected),
            "{description}: expected a diagnostic containing {expected:?}, got {reported:?}"
        );
    }
}

/// The shape checks are the spine seven derives share, so a rule added here
/// without a case is a rule seven derives stop enforcing together.
#[test]
fn every_shared_diagnostic_has_a_case() {
    const SOURCE: &str = include_str!("../common.rs");

    // These tests live in the file they count, and they name both
    // diagnostic constructors in the very strings they count with. So
    // counting stops where the implementation does.
    let implementation = SOURCE
        .split_once("\n#[cfg(test)]")
        .map_or(SOURCE, |(before, _)| before);

    let sites = implementation.matches("syn::Error::new(").count()
        + implementation.matches("meta.error(").count();
    assert_eq!(
        cases().len(),
        sites,
        "`common.rs` raises {sites} diagnostic(s) and {} have a case",
        cases().len()
    );
}
