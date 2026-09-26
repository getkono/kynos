use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields, FieldsNamed};

use super::{named_fields, reject_duplicate_names, unit_struct, wire_names};

/// The wire name `attribute`'s derive gives the one field of `declaration`.
///
/// Built from the whole item because the container's `rename_all` is part of
/// the answer, and a field alone does not carry it.
fn name_of(declaration: TokenStream2, attribute: &str) -> syn::Result<String> {
    let input = item(declaration);
    let fields = named_fields(&input, "QueryParams").expect("a struct with named fields");
    let mut names = wire_names(&input, fields, attribute)?;
    assert_eq!(names.len(), 1, "one field, one name");
    Ok(names.remove(0))
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

/// Which of the four sources a wire name comes from, over every combination
/// of the three that can be absent.
///
/// A sweep rather than examples: the rule is a precedence, and a precedence
/// is only wrong when two sources are present at once. Reading serde's own
/// `rename` and `rename_all` is what stops a type describing one field name
/// while serializing another, so which one wins when several are set is the
/// whole point. The order is serde's, and the `Schema` derive's.
#[test]
fn a_wire_name_prefers_the_kynos_rename_then_serdes_then_rename_all_then_the_identifier() {
    for rename_all in [false, true] {
        for kynos in [None, Some("from_kynos")] {
            for serde in [None, Some("from_serde")] {
                let container = rename_all.then(|| quote!(#[serde(rename_all = "camelCase")]));
                let kynos_attribute = kynos.map(|name| quote!(#[param(rename = #name)]));
                let serde_attribute = serde.map(|name| quote!(#[serde(rename = #name)]));
                let declaration = quote! {
                    #container
                    struct Holder {
                        #kynos_attribute
                        #serde_attribute
                        user_id: u64
                    }
                };

                let expected =
                    kynos
                        .or(serde)
                        .unwrap_or(if rename_all { "userId" } else { "user_id" });
                assert_eq!(
                    name_of(declaration, "param").expect("a wire name"),
                    expected,
                    "rename_all: {rename_all}, kynos: {kynos:?}, serde: {serde:?}"
                );
            }
        }
    }
}

/// `rename_all` is the container's, so every derive's attribute reads it --
/// a header name is where `kebab-case` is the convention rather than a style.
#[test]
fn rename_all_reaches_every_location_attribute() {
    for attribute in ["param", "header", "cookie"] {
        let declaration = quote! {
            #[serde(rename_all = "kebab-case")]
            struct Holder {
                x_request_id: String
            }
        };

        assert_eq!(
            name_of(declaration, attribute).expect("a wire name"),
            "x-request-id",
            "{attribute}"
        );
    }
}

/// An alias is refused however else the field is named, the Kynos `rename`
/// included: that `rename` settles the name the description carries, and
/// serde would still read the alias beside it.
#[test]
fn an_alias_is_refused_whatever_else_names_the_field() {
    for (shape, attributes) in [
        ("alone", quote!(#[serde(alias = "userId")])),
        (
            "beside the Kynos rename",
            quote!(#[param(rename = "id")] #[serde(alias = "userId")]),
        ),
        (
            "beside serde's rename",
            quote!(#[serde(rename = "id", alias = "userId")]),
        ),
    ] {
        let declaration = quote! {
            struct Holder {
                #attributes
                user_id: u64
            }
        };

        let error = name_of(declaration, "param").expect_err(shape);
        assert!(
            error.to_string().contains("a second wire name"),
            "{shape}: {error}"
        );
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
        let declaration = quote! {
            struct Holder {
                #[param(#skipped, rename = "chosen")]
                user_id: u64
            }
        };

        assert_eq!(
            name_of(declaration, "param").expect("a wire name"),
            "chosen",
            "{shape} must be stepped over, not consumed"
        );
    }
}

/// An attribute this derive does not model is not a mistake, so a key it
/// does not know is skipped rather than refused.
#[test]
fn an_unrecognized_key_alone_is_not_an_error() {
    let declaration = quote! {
        struct Holder {
            #[param(unknown = 1)]
            user_id: u64
        }
    };

    assert_eq!(
        name_of(declaration, "param").expect("a wire name"),
        "user_id"
    );
}

/// A Kynos attribute belonging to another derive is not this one's to read.
#[test]
fn only_the_named_attribute_is_consulted() {
    let declaration = quote! {
        struct Holder {
            #[header(rename = "X-Other")]
            user_id: u64
        }
    };

    assert_eq!(
        name_of(declaration, "param").expect("a wire name"),
        "user_id"
    );
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
// Long because it is one row per site, and `every_shared_diagnostic_has_a_case`
// counts them: splitting the table would split the list that count reads.
#[expect(clippy::too_many_lines)]
fn cases() -> Vec<(&'static str, syn::Result<()>, &'static str)> {
    fn shape(input: &DeriveInput) -> syn::Result<()> {
        named_fields(input, "QueryParams").map(|_| ())
    }
    fn unit(input: &DeriveInput) -> syn::Result<()> {
        unit_struct(input, "Tag", "names a group of operations")
    }
    fn named_by(declaration: TokenStream2) -> syn::Result<()> {
        name_of(declaration, "param").map(|_| ())
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
            named_by(quote!(
                struct Holder {
                    #[serde(rename(serialize = "a", deserialize = "b"))]
                    user_id: u64,
                }
            )),
            "two wire names",
        ),
        (
            "serde's alias, which gives one field a second wire name",
            named_by(quote!(
                struct Holder {
                    #[serde(alias = "userId")]
                    user_id: u64,
                }
            )),
            "a second wire name",
        ),
        (
            "serde's split rename_all, which gives every field two wire names",
            named_by(quote!(
                #[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
                struct Holder {
                    user_id: u64,
                }
            )),
            "split `rename_all`",
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
