use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields, FieldsNamed};

use super::{
    NameCase, doc_string, named_fields, reject_duplicate_names, reject_non_token_names,
    unit_struct, wire_names,
};

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

/// A description is CommonMark, so the indentation a doc comment's Markdown
/// depends on reaches it: an indented code block, a nested list item and a
/// two-space hard break each survive, and only the indentation every line
/// shares - rustdoc's one space after `///` - is removed.
#[test]
fn a_doc_comment_keeps_its_markdown_indentation() {
    let input = item(quote! {
        /// A user.
        ///
        /// Created with:
        ///
        ///     let user = User::new();
        ///
        /// - roles
        ///   - admin
        ///
        #[doc = " ends here  "]
        /// and resumes here.
        ///
        struct User;
    });

    assert_eq!(
        doc_string(&input.attrs).as_deref(),
        Some(
            "A user.\n\nCreated with:\n\n    let user = User::new();\n\n- roles\n  - admin\n\n\
             ends here  \nand resumes here."
        )
    );
}

/// Indentation is removed only where every line shares it: a doc attribute
/// written without rustdoc's leading space keeps the indentation of the lines
/// beside it, and the lines one attribute's text spans are lines of their own.
#[test]
fn only_the_indentation_every_line_shares_is_removed() {
    let input = item(quote! {
        #[doc = "Run it:"]
        #[doc = ""]
        #[doc = "    cargo run\n    cargo test"]
        struct Command;
    });

    assert_eq!(
        doc_string(&input.attrs).as_deref(),
        Some("Run it:\n\n    cargo run\n    cargo test")
    );
}

/// Which of the four sources a wire name comes from, over every combination
/// of the three that can be absent.
///
/// A sweep rather than examples: the rule is a precedence, and a precedence
/// is only wrong when two sources are present at once. Reading serde's own
/// `rename` and `rename_all` is what stops a type describing one field name
/// while serializing another, so which one wins when several are set is the
/// whole point. Below the Kynos `rename`, which only the parameter derives
/// read, the precedence is serde's, and the `Schema` derive's.
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

/// A split `rename_all` is refused even where its sides agree, unlike in the
/// `Schema` derive, which reads agreeing sides as the single form.
///
/// Beside the table rather than in it: the table's row proves the site fires,
/// and this proves it fires whatever the sides say.
#[test]
fn a_split_rename_all_is_refused_even_where_its_sides_agree() {
    let error = name_of(
        quote! {
            #[serde(rename_all(serialize = "camelCase", deserialize = "camelCase"))]
            struct Holder {
                user_id: u64
            }
        },
        "param",
    )
    .expect_err("a split `rename_all` whose sides agree");
    assert!(error.to_string().contains("split `rename_all`"), "{error}");
}

/// A key the attribute does not take is refused in every shape it may be
/// written in, `rename` behind it or not: skipped, a misspelled `rename` was a
/// name that silently fell back to the identifier.
#[test]
fn an_unrecognized_key_is_refused_whatever_its_shape() {
    for (shape, unknown) in [
        ("a named value", quote!(renam = "chosen")),
        ("a parenthesized group", quote!(unknown(a, b))),
        ("a bare path", quote!(unknown)),
        ("a qualified path", quote!(serde::rename = "chosen")),
    ] {
        for written in [quote!(#unknown), quote!(#unknown, rename = "chosen")] {
            let declaration = quote! {
                struct Holder {
                    #[param(#written)]
                    user_id: u64
                }
            };

            let error = name_of(declaration, "param").expect_err(shape);
            assert!(
                error
                    .to_string()
                    .contains("is not part of the `#[param(...)]` grammar"),
                "{shape}: {error}"
            );
        }
    }
}

/// `value` fixes a header's value, so `#[header]` takes it and the other
/// locations' attributes refuse it.
#[test]
fn value_is_a_header_key_alone() {
    for attribute in ["param", "cookie"] {
        let path = syn::Ident::new(attribute, proc_macro2::Span::call_site());
        let declaration = quote! {
            struct Holder {
                #[#path(value = "fixed")]
                user_id: u64
            }
        };

        let error = name_of(declaration, attribute).expect_err(attribute);
        assert!(
            error.to_string().contains("`value` is not part of"),
            "{attribute}: {error}"
        );
    }

    let declaration = quote! {
        struct Holder {
            #[header(value = "fixed")]
            user_id: ()
        }
    };
    assert_eq!(
        name_of(declaration, "header").expect("a wire name"),
        "user_id"
    );
}

/// A key is read once per field, across every list of its attribute, so a
/// second `rename` is refused rather than silently replacing the first.
#[test]
fn a_repeated_key_is_refused_within_and_across_lists() {
    for (shape, declaration) in [
        (
            "within one list",
            quote! {
                struct Holder {
                    #[param(rename = "first", rename = "second")]
                    user_id: u64
                }
            },
        ),
        (
            "across two lists",
            quote! {
                struct Holder {
                    #[param(rename = "first")]
                    #[param(rename = "second")]
                    user_id: u64
                }
            },
        ),
    ] {
        let error = name_of(declaration, "param").expect_err(shape);
        assert!(
            error.to_string().contains("`rename` is already set"),
            "{shape}: {error}"
        );
    }
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
    for case in [NameCase::Sensitive, NameCase::Insensitive] {
        assert!(reject_duplicate_names(&fields, &names, "parameter", case).is_ok());
    }
}

/// Names differing only in case are one name where the location folds case,
/// and two where it does not.
#[test]
fn case_decides_whether_two_spellings_clash() {
    let fields = named(quote!(
        struct Tracing {
            upper: String,
            lower: String,
        }
    ));
    let names = ["X-Id".to_owned(), "x-id".to_owned()];

    assert!(reject_duplicate_names(&fields, &names, "cookie", NameCase::Sensitive).is_ok());

    let error = reject_duplicate_names(&fields, &names, "header", NameCase::Insensitive)
        .expect_err("two spellings of one header name must be refused");
    let reported = error.to_string();
    assert!(
        reported.contains("two fields declare the header `x-id`"),
        "{reported}"
    );
    assert!(
        reported.contains("the first is `upper`, which spells it `X-Id`"),
        "{reported}"
    );
}

/// A token is one or more visible ASCII characters other than the RFC 9110
/// §5.6.2 delimiters. Swept over every ASCII character and one beyond it,
/// against that definition rather than the `tchar` list the check holds.
#[test]
fn a_name_is_accepted_exactly_when_it_is_a_token() {
    const DELIMITERS: &str = "\"(),/:;<=>?@[\\]{}";
    let fields = named(quote!(
        struct Holder {
            value: String,
        }
    ));

    for character in (0..=0x7f_u8).map(char::from).chain(['é']) {
        let token = character.is_ascii_graphic() && !DELIMITERS.contains(character);
        let names = [format!("a{character}b")];
        let outcome = reject_non_token_names(&fields, &names, "header", "a field name");
        assert_eq!(outcome.is_ok(), token, "{character:?}");
    }

    let error = reject_non_token_names(&fields, &[String::new()], "header", "a field name")
        .expect_err("an empty name is no token");
    assert!(error.to_string().contains("is empty"), "{error}");
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

    let error = reject_duplicate_names(&fields, &names, "parameter", NameCase::Sensitive)
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
                NameCase::Sensitive,
            ),
            "two fields declare the",
        ),
        (
            "a wire name outside the token grammar",
            reject_non_token_names(
                &duplicate,
                &["page".to_owned(), "x id".to_owned()],
                "header",
                "a field name",
            ),
            "contains ' '",
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

    let sites = SOURCE.matches("syn::Error::new(").count() + SOURCE.matches("meta.error(").count();
    assert_eq!(
        cases().len(),
        sites,
        "`common.rs` raises {sites} diagnostic(s) and {} have a case",
        cases().len()
    );
}
