//! `#[derive(MultipartForm)]`.
//!
//! No attribute of its own: a field is read from and written to the part named
//! as [`schema`](super::schema) names its property. A split `rename` or
//! `rename_all` whose sides differ is refused, since a part has one name.
//!
//! Parts convert through `FromPart` and `IntoPart`. Multiplicity is read
//! syntactically from the type: `Vec<T>` is one part per element, `Option<T>`
//! an optional part, anything else a required one.
//!
//! The decoder accepts what the `Schema` derive describes: a field's
//! `#[serde(default)]` fills a part left out, and `deny_unknown_fields`
//! refuses a part no field declares. A serde key the description honours and
//! a part cannot — a container `default`, `tag` or `transparent`, an `alias`,
//! any `skip`, a `flatten` — is refused, and so is a `Vec<T>` without `default`, whose empty value is
//! written as no part although its description requires one.

mod refusals;

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::{DeriveInput, Field, Ident, Type, spanned::Spanned};

use crate::derive::{
    common::{NameCase, named_fields, reject_duplicate_names, wrapped},
    schema::{
        attributes::{DefaultFrom, serde_default, serde_key_span},
        property_names,
    },
};

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let fields = named_fields(input, "MultipartForm")?;
    refusals::check(input, fields)?;
    let names = property_names(input, fields);
    reject_duplicate_names(fields, &names, "part", NameCase::Sensitive)?;

    let declared: Vec<Declared<'_>> = fields
        .named
        .iter()
        .zip(&names)
        .enumerate()
        .map(|(index, (field, name))| Declared::new(index, field, name))
        .collect::<syn::Result<_>>()?;

    // One binding per field, filled in a single pass over the parts.
    let bindings = declared.iter().map(Declared::binding);
    let closed = serde_key_span(&input.attrs, &["deny_unknown_fields"]).is_some();
    let dispatch = dispatch(&declared, closed);
    let reads = declared.iter().map(Declared::read);
    let idents = declared.iter().map(Declared::ident);
    let writes = declared.iter().map(Declared::write);
    // How many parts the fields write, so the list is allocated once.
    let count = declared
        .iter()
        .map(Declared::count)
        .reduce(|sum, count| quote!(#sum + #count))
        .unwrap_or_else(|| quote!(0));
    let moved = declared.iter().map(Declared::ident);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::kynos::extract::body::multipart::FromMultipart
            for #name #ty_generics #where_clause
        {
            fn from_parts(
                parts: ::std::vec::Vec<::kynos::extract::body::multipart::Part>,
            ) -> ::core::result::Result<Self, ::kynos::error::rejection::BodyRejection> {
                #(#bindings)*

                // A part naming no declared field is ignored, like an
                // undeclared query parameter, unless the form is closed.
                for part in parts {
                    #dispatch
                }

                #(#reads)*
                ::core::result::Result::Ok(Self { #(#idents),* })
            }
        }

        impl #impl_generics ::kynos::response::codec::multipart::IntoMultipart
            for #name #ty_generics #where_clause
        {
            fn into_parts(self) -> ::std::vec::Vec<::kynos::extract::body::multipart::Part> {
                let Self { #(#moved),* } = self;
                let mut parts: ::std::vec::Vec<::kynos::extract::body::multipart::Part> =
                    ::std::vec::Vec::with_capacity(#count);
                #(#writes)*
                parts
            }
        }
    })
}

/// Sorts one part into the field that declared its name, else drops it or,
/// in a `closed` form, refuses it; an `if` chain, since a `match` on
/// `part.name` could not move `part`.
fn dispatch(declared: &[Declared<'_>], closed: bool) -> TokenStream2 {
    let undeclared = if closed {
        quote! {{
            return ::core::result::Result::Err(
                ::kynos::error::rejection::BodyRejection::Schema {
                    failures: ::core::iter::FromIterator::from_iter([(
                        ::std::string::ToString::to_string(
                            &::kynos::schema::constraints::Pointer::root().member(&part.name),
                        ),
                        <::std::string::String as ::core::convert::From<_>>::from(
                            "the part is not declared",
                        ),
                    )]),
                },
            );
        }}
    } else {
        quote!({})
    };
    declared.iter().rev().fold(undeclared, |rest, field| {
        let gather = field.gather();
        let name = field.name;
        quote! {
            if part.name == #name {
                #gather
            } else #rest
        }
    })
}

/// How many parts one field is carried by, read from the field's type.
#[derive(Clone, Copy)]
enum Arity {
    /// Exactly one part, which the body must have carried.
    One,
    /// One part or none, which is what an `Option<T>` field declares.
    Optional,
    /// One part per element, which is what a `Vec<T>` field declares.
    Repeated,
}

/// One declared field, paired with the part name it occupies.
struct Declared<'a> {
    index: usize,
    field: &'a Field,
    name: &'a str,
    arity: Arity,
    /// The type one part becomes: the `T` of `Option<T>` or `Vec<T>`, else the
    /// field's own type.
    element: &'a Type,
    /// What fills the field when no part carries it, from its
    /// `#[serde(default)]`.
    default: Option<DefaultFrom>,
}

impl<'a> Declared<'a> {
    fn new(index: usize, field: &'a Field, name: &'a str) -> syn::Result<Self> {
        let (arity, element) = wrapped(&field.ty, "Option")
            .map(|inner| (Arity::Optional, inner))
            .or_else(|| wrapped(&field.ty, "Vec").map(|inner| (Arity::Repeated, inner)))
            .unwrap_or((Arity::One, &field.ty));
        let default = serde_default(&field.attrs);

        if matches!(arity, Arity::Repeated) && default.is_none() {
            return Err(syn::Error::new(
                field.ty.span(),
                "a `Vec` field is carried by no part when it is empty, and its description, \
                 without `default`, requires it. Mark it `#[serde(default)]`, so an empty list \
                 and a missing part are one value in both directions",
            ));
        }

        Ok(Self {
            index,
            field,
            name,
            arity,
            element,
            default,
        })
    }

    fn ident(&self) -> &Ident {
        self.field
            .ident
            .as_ref()
            .expect("a multipart form is a struct with named fields")
    }

    /// The local the parts naming this field are gathered into; numbered, since
    /// a raw identifier such as `r#type` does not concatenate.
    fn gathered(&self) -> Ident {
        format_ident!("__kynos_parts_{}", self.index)
    }

    /// One part converted into this field's element, spanned at the field so
    /// a missing `FromPart` is reported there.
    fn convert(&self) -> TokenStream2 {
        let element = self.element;
        quote_spanned! {self.field.ty.span()=>
            <#element as ::kynos::extract::body::multipart::FromPart>::from_part(part)
        }
    }

    /// The local's declaration. A single part is held as it arrived; a
    /// repeated field's are converted as they arrive, keeping the first
    /// failure to report in declaration order.
    fn binding(&self) -> TokenStream2 {
        let gathered = self.gathered();
        let element = self.element;
        match self.arity {
            Arity::One | Arity::Optional => quote! {
                let mut #gathered: ::core::option::Option<
                    ::kynos::extract::body::multipart::Part,
                > = ::core::option::Option::None;
            },
            Arity::Repeated => quote! {
                let mut #gathered: ::core::result::Result<
                    ::std::vec::Vec<#element>,
                    ::kynos::error::rejection::BodyRejection,
                > = ::core::result::Result::Ok(::std::vec::Vec::new());
            },
        }
    }

    /// Takes in one part naming this field. Only the first is kept for a
    /// single part, and nothing more is converted once a repeated one fails.
    fn gather(&self) -> TokenStream2 {
        let gathered = self.gathered();
        match self.arity {
            Arity::One | Arity::Optional => quote! {
                if ::core::option::Option::is_none(&#gathered) {
                    #gathered = ::core::option::Option::Some(part);
                }
            },
            Arity::Repeated => {
                let convert = self.convert();
                quote! {
                    if let ::core::result::Result::Ok(values) = &mut #gathered {
                        match #convert {
                            ::core::result::Result::Ok(value) => values.push(value),
                            ::core::result::Result::Err(rejection) => {
                                #gathered = ::core::result::Result::Err(rejection);
                            }
                        }
                    }
                }
            }
        }
    }

    /// How many parts this field writes.
    fn count(&self) -> TokenStream2 {
        let ident = self.ident();
        match self.arity {
            Arity::One => quote!(1usize),
            Arity::Optional => quote! {
                <::core::primitive::usize as ::core::convert::From<::core::primitive::bool>>::from(
                    ::core::option::Option::is_some(&#ident),
                )
            },
            Arity::Repeated => quote!(#ident.len()),
        }
    }

    /// Binds this field from what was gathered for it.
    fn read(&self) -> TokenStream2 {
        let ident = self.ident();
        let gathered = self.gathered();
        let span = self.field.ty.span();
        let name = self.name;

        let one = self.convert();
        let one = quote!(#one?);

        // What the field is when no part carries it: its `default`, else
        // what its arity says absence is.
        let missing = match (&self.default, self.arity) {
            (Some(DefaultFrom::Default), _) => {
                quote_spanned! {span=> ::core::default::Default::default() }
            }
            (Some(DefaultFrom::Path(path)), _) => quote_spanned! {path.span()=> #path() },
            (None, Arity::One) => quote! {
                return ::core::result::Result::Err(
                    ::kynos::error::rejection::BodyRejection::Schema {
                        failures: ::core::iter::FromIterator::from_iter([(
                            ::std::string::ToString::to_string(
                                &::kynos::schema::constraints::Pointer::root().member(#name),
                            ),
                            <::std::string::String as ::core::convert::From<_>>::from(
                                "the part is required",
                            ),
                        )]),
                    },
                )
            },
            (None, Arity::Optional) => quote!(::core::option::Option::None),
            // `Declared::new` refuses a `Vec` without `default`.
            (None, Arity::Repeated) => quote!(::std::vec::Vec::new()),
        };

        match self.arity {
            // Only the first part under this name was kept.
            Arity::One => quote! {
                let #ident = match #gathered {
                    ::core::option::Option::Some(part) => #one,
                    ::core::option::Option::None => #missing,
                };
            },
            Arity::Optional => quote! {
                let #ident = match #gathered {
                    ::core::option::Option::Some(part) => ::core::option::Option::Some(#one),
                    ::core::option::Option::None => #missing,
                };
            },
            Arity::Repeated => quote! {
                let #ident = match #gathered {
                    ::core::result::Result::Ok(values) if values.is_empty() => #missing,
                    ::core::result::Result::Ok(values) => values,
                    ::core::result::Result::Err(rejection) => {
                        return ::core::result::Result::Err(rejection);
                    }
                };
            },
        }
    }

    /// Writes this field back as the parts it was read from.
    fn write(&self) -> TokenStream2 {
        let ident = self.ident();
        let name = self.name;
        let span = self.field.ty.span();
        let element = self.element;

        let push = |value: TokenStream2| {
            quote_spanned! {span=>
                parts.push(
                    <#element as ::kynos::response::codec::multipart::IntoPart>::into_part(
                        #value, #name,
                    ),
                );
            }
        };

        match self.arity {
            Arity::One => push(quote!(#ident)),
            Arity::Optional => {
                let push = push(quote!(value));
                quote! {
                    if let ::core::option::Option::Some(value) = #ident {
                        #push
                    }
                }
            }
            Arity::Repeated => {
                let push = push(quote!(value));
                quote! {
                    for value in #ident {
                        #push
                    }
                }
            }
        }
    }
}
