//! Describing a parameter group's fields: the OpenAPI parameters, or a
//! response's headers, each field's schema resolved under its bounds.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use super::{List, Param, default};
use crate::{
    derive::schema::{aliases::named_string, attributes::constraints},
    doc::doc_string,
};

/// Puts the field's `#[schema(...)]` bounds on the resolved `schema`, as the
/// `Schema` derive puts them on a property; [`check_field`](super::check_field)
/// enforces them.
fn bounds(param: &Param<'_>) -> Option<TokenStream2> {
    constraints(param.field).map(|constraints| quote!(let schema = #constraints.apply(schema);))
}

/// The schema of the one value the field carries, or a fixed parameter's
/// `const`.
///
/// An `Option<T>` field is described by `T`'s schema: no parameter location
/// can spell `null`, so its optionality is `required: false` alone. A list is
/// described by its `Vec<T>`'s, an array of `T`.
fn schema(param: &Param<'_>) -> TokenStream2 {
    let ty = param.optional().unwrap_or(param.ty());
    param.fixed.as_ref().map_or_else(
        || quote!(registry.resolve::<#ty>()),
        |value| named_string(std::slice::from_ref(value)),
    )
}

/// Whether a list parameter is required, as an expression: a list's absence is
/// its emptiness, so exactly when its field's `min_items` is at least one,
/// read from the `Constraints` the field builds, since a count may be a
/// `const` the derive cannot evaluate.
fn required_list(param: &Param<'_>) -> TokenStream2 {
    constraints(param.field).map_or_else(
        || quote!(false),
        |constraints| {
            quote! {
                ::core::option::Option::is_some_and((#constraints).min_items, |count| count > 0)
            }
        },
    )
}

/// The `parameters` body: one OpenAPI parameter per field, in declaration
/// order, each schema resolved through the registry under its field's bounds.
///
/// `always_required` is the path location's, which the OpenAPI Parameter Object
/// requires to be `required: true` whatever the Rust type says.
pub(crate) fn parameters_body(
    params: &[Param<'_>],
    location: &TokenStream2,
    always_required: bool,
) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let schema = schema(param);
        let name = &param.name;
        let required = if param.list.is_some() {
            required_list(param)
        } else {
            let required = always_required || (param.optional().is_none() && !param.has_default());
            quote!(#required)
        };
        let constrained = bounds(param);
        let defaulted = default::described(param);
        let described = doc_string(&param.field.attrs).map(|text| {
            quote!(parameter.description = ::core::option::Option::Some(
                <::std::string::String as ::core::convert::From<_>>::from(#text)
            );)
        });
        // Written out though `form` with `explode` is the query default, so
        // the document says how the list is carried without a reader knowing
        // the default.
        let styled = param.list.map(|list| {
            let explode = list == List::Exploded;
            quote! {
                let parameter =
                    parameter.with_style(::kynos::openapi::Style::Form, #explode);
            }
        });

        quote! {
            parameters.push({
                let schema = #schema;
                #constrained
                #defaulted
                let mut parameter =
                    ::kynos::openapi::Parameter::new(#name, #location, schema);
                parameter.required = ::core::option::Option::Some(#required);
                #described
                #styled
                parameter
            });
        }
    });

    // A group of fixed headers alone resolves nothing through `registry`.
    quote! {
        let _ = &registry;
        let mut parameters = ::std::vec::Vec::new();
        #(#entries)*
        parameters
    }
}

/// The `response_headers` body: the same fields, as a `headers` map.
pub(crate) fn response_headers_body(params: &[Param<'_>]) -> TokenStream2 {
    let entries = params.iter().map(|param| {
        let schema = schema(param);
        let name = &param.name;
        let required = param.optional().is_none();
        let constrained = bounds(param);
        let described = doc_string(&param.field.attrs).map(|text| {
            quote!(header.description = ::core::option::Option::Some(
                <::std::string::String as ::core::convert::From<_>>::from(#text)
            );)
        });

        quote! {
            headers.insert(<::std::string::String as ::core::convert::From<_>>::from(#name), {
                let schema = #schema;
                #constrained
                let mut header = ::kynos::openapi::Header::new(schema);
                header.required = ::core::option::Option::Some(#required);
                #described
                ::kynos::openapi::RefOr::Item(header)
            });
        }
    });

    quote! {
        let _ = &registry;
        let mut headers = ::kynos::openapi::Map::new();
        #(#entries)*
        headers
    }
}
