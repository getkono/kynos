//! The names serde reads a named field or a variant under, and how a schema
//! says so.
//!
//! serde refuses a document naming two of a member's names as a duplicate, so
//! each name is a property and an `allOf` entry bounds how many are present. A
//! name two variants claim is read as the first alone.

use super::{
    Container, Field, TokenStream2, Variant, close, field_read_name, is_required, quote,
    shape::member_schema, skip_value, string_value, variant_read_name,
};

/// Every name serde reads a named field under: its read name first, then each
/// `alias` in the order written, each once. `rename_all` does not reach an alias.
pub(super) fn read_names(field: &Field, container: &Container) -> Vec<String> {
    names(field_read_name(field, container), &field.attrs)
}

/// The same for a variant: its read name first, then each distinct `alias`.
pub(super) fn variant_names(variant: &Variant, container: &Container) -> Vec<String> {
    names(variant_read_name(variant, container), &variant.attrs)
}

/// The names serde reads as each of `variants`, in order: its
/// [`variant_names`] less any an earlier one claims, since serde reads a name
/// as the first variant claiming it.
pub(super) fn variants_read_names(
    variants: &[&Variant],
    container: &Container,
) -> Vec<Vec<String>> {
    let mut claimed: Vec<String> = Vec::new();
    variants
        .iter()
        .map(|variant| {
            let read: Vec<String> = variant_names(variant, container)
                .into_iter()
                .filter(|name| !claimed.contains(name))
                .collect();
            claimed.extend(read.iter().cloned());
            read
        })
        .collect()
}

/// The first of `variants` whose own name an earlier one claims, with that
/// earlier variant. The own name is the read name, which `reject_split_rename`
/// makes the written one too.
pub(super) fn shadowed_variant<'a>(
    variants: &[&'a Variant],
    container: &Container,
) -> Option<(&'a Variant, &'a Variant)> {
    variants.iter().enumerate().find_map(|(index, later)| {
        let wire = variant_read_name(later, container);
        variants[..index]
            .iter()
            .find(|earlier| variant_names(earlier, container).contains(&wire))
            .map(|earlier| (*later, *earlier))
    })
}

/// `wire`, then each `alias` in `attrs` not already listed, in the order
/// written.
fn names(wire: String, attrs: &[syn::Attribute]) -> Vec<String> {
    let mut names = vec![wire];
    for attr in attrs {
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
/// One name: a property, in `required` per [`is_required`]. Several: a property
/// under each, bounded to exactly one (required) or at most one (optional).
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

/// A string that is one of `names`: a `const` where there is one, an `enum`
/// otherwise. What a tag property and an externally tagged unit variant are.
pub(super) fn named_string(names: &[String]) -> TokenStream2 {
    let value = if let [name] = names {
        quote!(constant.const_value = ::core::option::Option::Some(::core::convert::Into::into(#name));)
    } else {
        quote! {
            constant.enumeration = ::core::option::Option::Some(::std::vec![
                #(::core::convert::Into::into(#names)),*
            ]);
        }
    };
    quote! {
        {
            let mut constant = ::kynos::openapi::SchemaObject::default();
            constant.ty = ::core::option::Option::Some(
                ::kynos::openapi::model::schema::types::TypeSet::One(
                    ::kynos::openapi::model::schema::types::SchemaType::String,
                ),
            );
            #value
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(constant))
        }
    }
}

/// An externally tagged branch keyed by the variant's `names`: `payload` under
/// each, present under exactly one, since serde reads the branch as one entry,
/// and closed to every other key.
pub(super) fn keyed(names: &[String], payload: &TokenStream2) -> TokenStream2 {
    let bound = if let [name] = names {
        quote! {
            keywords.required =
                ::core::option::Option::Some(::std::vec![::std::string::String::from(#name)]);
        }
    } else {
        let exactly = exactly_one(names);
        quote!(keywords.all_of = ::core::option::Option::Some(::std::vec![#exactly]);)
    };
    close(&quote! {
        {
            let mut keywords = ::kynos::openapi::SchemaObject::default();
            keywords.ty = ::core::option::Option::Some(
                ::kynos::openapi::model::schema::types::TypeSet::One(
                    ::kynos::openapi::model::schema::types::SchemaType::Object,
                ),
            );
            let payload = #payload;
            #(
                keywords
                    .properties
                    .insert(::std::string::String::from(#names), ::core::clone::Clone::clone(&payload));
            )*
            #bound
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
        }
    })
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
