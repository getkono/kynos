//! Emitting the endpoint type that accompanies a handler.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{ItemFn, LitStr};

use crate::route::{
    args::RouteArgs,
    attrs::{doc_lines, is_deprecated, split_doc},
    uri::endpoint_uri_impl,
};

/// Emits the endpoint type alongside the original handler.
pub(crate) fn emit(method: &str, args: &RouteArgs, function: &ItemFn) -> TokenStream2 {
    let raw_path = args.path.value();

    // The document model's parser, so there is one notion of a valid template.
    let variables = match kynos_openapi::PathTemplate::parse(raw_path.clone()) {
        Ok(template) => template
            .variables()
            .iter()
            .map(String::clone)
            .collect::<Vec<_>>(),
        Err(error) => {
            return syn::Error::new(args.path.span(), error.to_string()).to_compile_error();
        }
    };

    let name = &function.sig.ident;
    let visibility = &function.vis;
    let (summary, description) = split_doc(&doc_lines(function));
    let deprecated = is_deprecated(function);

    let operation_id = args
        .operation_id
        .as_ref()
        .map_or_else(|| name.to_string(), LitStr::value);

    let summary = option_str(summary.as_deref());
    let description = option_str(description.as_deref());
    let uri_impl = match endpoint_uri_impl(function, &raw_path, &variables) {
        Ok(uri_impl) => uri_impl,
        Err(error) => return error.to_compile_error(),
    };
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
            compile_error!(
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

    quote! {
        #function

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, Default)]
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
    }
}

fn option_str(value: Option<&str>) -> TokenStream2 {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |text| quote!(::core::option::Option::Some(#text)),
    )
}
