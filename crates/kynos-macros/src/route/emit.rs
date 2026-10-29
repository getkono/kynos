//! Emitting the endpoint type that accompanies a handler.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{ItemFn, LitStr};

use crate::{
    derive::common::optional,
    doc::{doc_texts, is_deprecated},
    route::{args::RouteArgs, attrs::split_doc, uri::endpoint_uri_impl},
};

/// Emits the endpoint type alongside the original handler.
pub(crate) fn emit(method: &str, args: &RouteArgs, function: &ItemFn) -> syn::Result<TokenStream2> {
    let raw_path = args.path.value();

    // The document model's parser, so there is one notion of a valid template.
    let variables = kynos_openapi::PathTemplate::parse(raw_path.clone())
        .map_err(|error| syn::Error::new(args.path.span(), error.to_string()))?
        .variables()
        .to_vec();

    let name = &function.sig.ident;
    let visibility = &function.vis;
    let (summary, description) = split_doc(&doc_texts(&function.attrs));
    // `Operation.deprecated`, read as a schema reads it.
    let deprecated = is_deprecated(&function.attrs);

    let operation_id = args
        .operation_id
        .as_ref()
        .map_or_else(|| name.to_string(), LitStr::value);

    let summary = optional(summary.as_deref());
    let description = optional(description.as_deref());
    let uri_impl = endpoint_uri_impl(function, &raw_path, &variables)?;
    let variables = variables.iter().map(String::as_str);

    // A braced struct occupies only the type namespace, so it shares the
    // handler function's name without shadowing it.
    let endpoint = format_ident!("{name}");
    let panic_policy = if args.catch_panics {
        quote!(::kynos::middleware::catch_panic::Catch)
    } else {
        quote!(::kynos::middleware::catch_panic::Propagate)
    };
    let panic_strategy_check = args.catch_panics.then(|| {
        quote! {
            #[cfg(panic = "abort")]
            ::core::compile_error!(
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
    });
    // A `DeclaredTag` carries the name and the thunk documenting it, so
    // `from_meta` registers both from this constant; `of` carries the `Tag` bound.
    let tags = args.tag.as_ref().map_or_else(
        || quote!(&[]),
        |tag| quote!(&[::kynos::router::operation::DeclaredTag::of::<#tag>()]),
    );

    Ok(quote! {
        #function

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #[derive(::core::clone::Clone, ::core::marker::Copy, ::core::fmt::Debug, ::core::default::Default)]
        #visibility struct #endpoint {}

        impl ::kynos::router::endpoint::meta::EndpointMeta for #endpoint {
            type PanicPolicy = #panic_policy;

            const METHOD: &'static str = #method;
            const PATH: &'static str = #raw_path;
            const PATH_VARIABLES: &'static [&'static str] = &[#(#variables),*];
            const OPERATION_ID: &'static str = #operation_id;
            const SUMMARY: ::core::option::Option<&'static str> = #summary;
            const DESCRIPTION: ::core::option::Option<&'static str> = #description;
            const DEPRECATED: bool = #deprecated;
            const TAGS: &'static [::kynos::router::operation::DeclaredTag] = #tags;
        }

        #uri_impl

        #panic_strategy_check
    })
}
