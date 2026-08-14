//! Shape checks and name handling shared by the derives.
//!
//! Every diagnostic here carries the span of the offending item and names the
//! tool that does work, rather than the trait that refused it. A user should
//! never have to read a Kynos internal to understand why their type was
//! rejected.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Field, Fields, FieldsNamed, LitStr, spanned::Spanned};

/// The named fields of a struct, or a diagnostic naming what was found instead.
pub(crate) fn named_fields<'a>(
    input: &'a DeriveInput,
    derive: &str,
) -> syn::Result<&'a FieldsNamed> {
    match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => Ok(fields),
        Data::Struct(DataStruct { fields, .. }) => Err(syn::Error::new(
            fields.span(),
            format!(
                "`{derive}` describes a group of named values, so it needs a struct with named fields"
            ),
        )),
        Data::Enum(data) => Err(syn::Error::new(
            data.enum_token.span(),
            format!("`{derive}` describes a group of named values, which an enum is not"),
        )),
        Data::Union(data) => Err(syn::Error::new(
            data.union_token.span(),
            format!("`{derive}` cannot describe a union"),
        )),
    }
}

/// Checks that the input is a struct with no fields.
pub(crate) fn unit_struct(input: &DeriveInput, derive: &str, purpose: &str) -> syn::Result<()> {
    match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Unit,
            ..
        }) => Ok(()),
        Data::Struct(DataStruct { fields, .. }) if fields.is_empty() => Ok(()),
        Data::Struct(DataStruct { fields, .. }) => Err(syn::Error::new(
            fields.span(),
            format!("`{derive}` marks a type that {purpose}, so it carries no fields"),
        )),
        Data::Enum(data) => Err(syn::Error::new(
            data.enum_token.span(),
            format!("`{derive}` marks a type that {purpose}, so it must be a unit struct"),
        )),
        Data::Union(data) => Err(syn::Error::new(
            data.union_token.span(),
            format!("`{derive}` marks a type that {purpose}, so it must be a unit struct"),
        )),
    }
}

/// Skips the value of the nested-meta item just matched.
///
/// Consuming `meta.input` wholesale would swallow every *later* item too, so
/// an attribute would silently lose everything after its first unrecognized
/// key. This takes exactly one `= value` or one `(...)` group and leaves the
/// rest of the list to the loop.
pub(crate) fn skip_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _: syn::Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let _ = content.parse::<proc_macro2::TokenStream>()?;
    }
    // A bare path such as `default` or `untagged` has no value to skip.
    Ok(())
}

/// The wire name of a field: its `rename` if it has one, else its identifier.
///
/// Both the Kynos attribute and serde's are consulted, in that order, so that a
/// type already carrying `#[serde(rename = "...")]` does not have to repeat
/// itself — and cannot end up describing one name while serializing another.
pub(crate) fn wire_name(field: &Field, attribute: &str) -> syn::Result<String> {
    if let Some(renamed) = kynos_rename(field, attribute)? {
        return Ok(renamed);
    }
    if let Some(renamed) = serde_rename(field)? {
        return Ok(renamed);
    }
    Ok(field
        .ident
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default())
}

/// The `rename = "..."` inside a Kynos attribute.
///
/// Strict about its own key and silent about every other, since the attribute
/// grammar grows and a key this derive does not yet model is not a mistake.
fn kynos_rename(field: &Field, attribute: &str) -> syn::Result<Option<String>> {
    let mut found = None;
    for attr in &field.attrs {
        if !attr.path().is_ident(attribute) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                found = Some(meta.value()?.parse::<LitStr>()?.value());
            } else {
                skip_value(&meta)?;
            }
            Ok(())
        })?;
    }
    Ok(found)
}

/// The `rename = "..."` inside `#[serde(...)]`.
///
/// Reading serde's own attribute is what stops a type describing one field
/// name while serializing another. serde also has a split form,
/// `rename(serialize = "a", deserialize = "b")`, which describes two names
/// where a parameter can have one — that is rejected rather than guessed at,
/// because guessing is the failure this whole function exists to prevent.
fn serde_rename(field: &Field) -> syn::Result<Option<String>> {
    let mut found = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("rename") {
                return skip_value(&meta);
            }
            if meta.input.peek(syn::token::Paren) {
                return Err(meta.error(
                    "a split `rename` gives this field two wire names, and a description can \
                     carry one. Say which with `rename = \"...\"`, or name it explicitly in the \
                     Kynos attribute",
                ));
            }
            found = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        })?;
    }
    Ok(found)
}

/// A `const NAMES` item listing the wire names of every field, in order.
pub(crate) fn names_const(names: &[String]) -> TokenStream2 {
    let literals = names
        .iter()
        .map(|name| LitStr::new(name, Span::call_site()));
    quote! {
        const NAMES: &'static [&'static str] = &[#(#literals),*];
    }
}

/// Rejects two fields that would occupy the same wire name.
///
/// Left to the derive rather than to validation because the span is here: a
/// duplicate reported against the emitted document names neither field.
pub(crate) fn reject_duplicate_names(
    fields: &FieldsNamed,
    names: &[String],
    kind: &str,
) -> syn::Result<()> {
    for (index, name) in names.iter().enumerate() {
        if let Some(earlier) = names[..index].iter().position(|seen| seen == name) {
            let field = fields
                .named
                .iter()
                .nth(index)
                .expect("index came from the same list");
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "two fields declare the {kind} `{name}`; the first is `{}`",
                    fields
                        .named
                        .iter()
                        .nth(earlier)
                        .and_then(|field| field.ident.as_ref())
                        .map_or_else(String::new, ToString::to_string)
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream as TokenStream2;
    use quote::quote;
    use syn::{Data, DataStruct, DeriveInput, Fields, FieldsNamed};

    use super::{named_fields, reject_duplicate_names, unit_struct, wire_name};

    /// The single named field of a struct wrapped around `declaration`.
    ///
    /// `Field` has no `Parse`, and a field only means anything inside the item
    /// that holds it, so the wrapper is the shortest honest way to build one.
    fn only_field(declaration: TokenStream2) -> syn::Field {
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
                let field = only_field(quote! {
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
            let field = only_field(quote! {
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
        let field = only_field(quote! {
            #[param(unknown = 1)]
            user_id: u64
        });

        assert_eq!(wire_name(&field, "param").expect("a wire name"), "user_id");
    }

    /// A Kynos attribute belonging to another derive is not this one's to read.
    #[test]
    fn only_the_named_attribute_is_consulted() {
        let field = only_field(quote! {
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
                    &only_field(quote! {
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
        const SOURCE: &str = include_str!("common.rs");

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
}
