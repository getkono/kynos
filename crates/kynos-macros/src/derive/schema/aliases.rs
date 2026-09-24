//! The names serde reads a named field under, and how an object says so.
//!
//! serde reads a field under its wire name or under any `#[serde(alias)]` it
//! carries, and refuses a document naming two of them as a duplicate field.
//! So every name is a property under the field's schema, and where there is
//! more than one, an `allOf` entry bounds how many of them may be present.

use super::{
    Container, Field, TokenStream2, field_name, is_required, quote, shape::member_schema,
    skip_value, string_value,
};

/// Every name serde reads a named field under: its wire name first, then each
/// `alias` in the order written, each once.
///
/// An alias is the literal name serde reads, since `rename_all` does not reach
/// it, and one repeating a name already listed adds nothing, as serde reads its
/// aliases as a set.
pub(super) fn read_names(field: &Field, container: &Container) -> Vec<String> {
    let mut names = vec![field_name(field, container)];
    for attr in &field.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        // Shape errors in serde's own attribute are serde's to report.
        let _ = attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("alias") {
                return skip_value(&meta);
            }
            if let Some(alias) = string_value(&meta)?.filter(|alias| !names.contains(alias)) {
                names.push(alias);
            }
            Ok(())
        });
    }
    names
}

/// What a described, unflattened named field adds to the object carrying it.
///
/// A field serde reads under one name is a property, listed in `required`
/// where [`is_required`] says so. One it reads under several is a property
/// under each, and required means present under exactly one name, a `oneOf`
/// over `required`; optional means present under at most one, a `not` over
/// every pair. Neither lists the field in the object's own `required`.
pub(super) fn property(field: &Field, container: &Container) -> TokenStream2 {
    let names = read_names(field, container);
    let schema = member_schema(field);
    let required = is_required(field, container);

    if let [wire] = names.as_slice() {
        let require = required.then(|| quote!(required.push(::std::string::String::from(#wire));));
        return quote! {
            keywords.properties.insert(::std::string::String::from(#wire), #schema);
            #require
        };
    }

    let bound = if required {
        exactly_one(&names)
    } else {
        at_most_one(&names)
    };
    quote! {
        {
            let schema = #schema;
            #(
                keywords
                    .properties
                    .insert(::std::string::String::from(#names), ::core::clone::Clone::clone(&schema));
            )*
            let bound = #bound;
            keywords
                .all_of
                .get_or_insert_with(::std::vec::Vec::new)
                .push(bound);
        }
    }
}

/// `oneOf` a `required` per name: present under exactly one.
fn exactly_one(names: &[String]) -> TokenStream2 {
    let branches = names.iter().map(|name| requiring(&[name]));
    quote! {
        {
            let mut exactly = ::kynos::openapi::SchemaObject::default();
            exactly.one_of = ::core::option::Option::Some(::std::vec![#(#branches),*]);
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(exactly))
        }
    }
}

/// `not` a `required` naming two of the names, over `anyOf` where there are
/// several pairs: present under at most one.
fn at_most_one(names: &[String]) -> TokenStream2 {
    let pairs: Vec<TokenStream2> = names
        .iter()
        .enumerate()
        .flat_map(|(index, first)| {
            names[index + 1..]
                .iter()
                .map(move |second| requiring(&[first, second]))
        })
        .collect();
    let refused = if let [pair] = pairs.as_slice() {
        pair.clone()
    } else {
        quote! {
            {
                let mut either = ::kynos::openapi::SchemaObject::default();
                either.any_of = ::core::option::Option::Some(::std::vec![#(#pairs),*]);
                ::kynos::openapi::Schema::Object(::std::boxed::Box::new(either))
            }
        }
    };
    quote! {
        {
            let mut apart = ::kynos::openapi::SchemaObject::default();
            apart.not = ::core::option::Option::Some(::std::boxed::Box::new(#refused));
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(apart))
        }
    }
}

/// A schema asserting only that each of `names` is present.
fn requiring(names: &[&String]) -> TokenStream2 {
    quote! {
        {
            let mut present = ::kynos::openapi::SchemaObject::default();
            present.required = ::core::option::Option::Some(
                ::std::vec![#(::std::string::String::from(#names)),*],
            );
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(present))
        }
    }
}
