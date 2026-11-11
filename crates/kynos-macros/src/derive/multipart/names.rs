//! Which declared field a part's name selects, decided twice: by
//! `FromMultipart::admit` before the part is read, and by `from_parts` once it
//! has been. Both give a part no field declares the same answer.

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use super::Declared;

/// The 422 a closed form answers a part named `name` with, which no field
/// declares.
fn undeclared_refusal(name: &TokenStream2) -> TokenStream2 {
    quote! {
        ::kynos::error::rejection::BodyRejection::Schema {
            failures: ::core::iter::FromIterator::from_iter([(
                ::std::string::ToString::to_string(
                    &::kynos::schema::constraints::Pointer::root().member(#name),
                ),
                <::std::string::String as ::core::convert::From<_>>::from(
                    "the part is not declared",
                ),
            )]),
            // A part no field reads is the form's shape, not a bound.
            type_uri: ::core::option::Option::None,
        }
    }
}

/// `FromMultipart::admit`: a declared part is read under its element's
/// `FromPart::LIMIT`, and any other is dropped unread or, in a `closed` form,
/// refused before it is read.
pub(super) fn admit(declared: &[Declared<'_>], closed: bool) -> TokenStream2 {
    let otherwise = if closed {
        let refusal = undeclared_refusal(&quote!(name));
        quote!(::core::result::Result::Err(#refusal))
    } else {
        quote!(::core::result::Result::Ok(
            ::kynos::extract::body::multipart::Admission::Skip
        ))
    };
    let arms = declared.iter().map(|field| {
        let name = field.name;
        let element = field.element;
        let limit = quote_spanned! {field.field.ty.span()=>
            <#element as ::kynos::extract::body::multipart::FromPart>::LIMIT
        };
        quote! {
            #name => ::core::result::Result::Ok(
                ::kynos::extract::body::multipart::Admission::Read { limit: #limit },
            ),
        }
    });
    quote! {
        fn admit(
            name: &::core::primitive::str,
        ) -> ::core::result::Result<
            ::kynos::extract::body::multipart::Admission,
            ::kynos::error::rejection::BodyRejection,
        > {
            match name {
                #(#arms)*
                _ => #otherwise,
            }
        }
    }
}

/// Sorts one part into the field that declared its name, else drops it or,
/// in a `closed` form, refuses it; an `if` chain, since a `match` on
/// `part.name` could not move `part`.
pub(super) fn dispatch(declared: &[Declared<'_>], closed: bool) -> TokenStream2 {
    let undeclared = if closed {
        let refusal = undeclared_refusal(&quote!(&part.name));
        quote! {{
            return ::core::result::Result::Err(#refusal);
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
