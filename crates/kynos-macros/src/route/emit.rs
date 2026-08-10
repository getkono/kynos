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

    // Reuse the document model's parser rather than reimplementing it here: two
    // notions of "valid path template" that could disagree is exactly the kind
    // of drift this framework exists to prevent.
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

    // A braced struct occupies only the type namespace, so it can share a name
    // with the handler function rather than shadowing it. `routes!` refers to
    // the type; callers and unit tests keep calling the function.
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
    let tag_note = args.tag.as_ref().map(|tag| {
        quote! {
            const _: fn() = || {
                fn assert_tag<T: ::kynos::router::operation::Tag>() {}
                assert_tag::<#tag>();
            };
        }
    });

    quote! {
        #function

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, Default)]
        #visibility struct #endpoint {}

        impl ::kynos::router::endpoint::EndpointMeta for #endpoint {
            type PanicPolicy = #panic_policy;

            const METHOD: &'static str = #method;
            const PATH: &'static str = #raw_path;
            const PATH_VARIABLES: &'static [&'static str] = &[#(#variables),*];
            const OPERATION_ID: &'static str = #operation_id;
            const SUMMARY: ::core::option::Option<&'static str> = #summary;
            const DESCRIPTION: ::core::option::Option<&'static str> = #description;
            const DEPRECATED: bool = #deprecated;
        }

        #uri_impl

        #tag_note
        #panic_strategy_check
    }
}

fn option_str(value: Option<&str>) -> TokenStream2 {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |text| quote!(::core::option::Option::Some(#text)),
    )
}
