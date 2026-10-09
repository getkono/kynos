//! `#[derive(ApiError)]`.
//!
//! ```text
//! #[problem( base = "<URI prefix>" )]              on the type, optional
//! #[problem( <member> [, <member>]* )]             on each variant, or on a struct
//! #[problem(extension)]                            on a named field, optional
//!
//! member := status = <400..=599>                   required, exactly once
//!         | title = "<human-readable summary>"
//!         | type = "<absolute URI>"
//! ```
//!
//! `status` is read once into `statuses()`, the `ShortCircuit` const and the
//! `Responses` keys, so the three agree. `base` is the prefix a variant without
//! its own `type` hangs its slug under. `detail` comes from `Display`.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Fields, Ident, LitInt, LitStr, parse_macro_input,
    spanned::Spanned,
};

use crate::derive::common::{doc_string, skip_value};

/// The range a problem detail's status may fall in: RFC 9457 errors, 4xx and
/// 5xx.
const STATUS_RANGE: std::ops::RangeInclusive<u16> = 400..=599;

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if let Data::Union(data) = &input.data {
        return Err(syn::Error::new(
            data.union_token.span(),
            "`ApiError` cannot describe a union",
        ));
    }

    let failures = failures(input)?;
    let statuses = distinct_statuses(&failures);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // `detail` comes from `Display`; asserted here rather than bounded so the
    // diagnostic lands on the error type, not the handler returning it.
    let display = quote! {
        const _: () = {
            #[allow(dead_code)]
            fn detail_comes_from_display #impl_generics () #where_clause {
                fn is_display<T: ::core::fmt::Display + ?Sized>() {}
                is_display::<#name #ty_generics>();
            }
        };
    };

    let problem = into_problem(&failures);
    let responses = responses(&failures, &statuses);

    // `Responses` and `into_problem` come from the same declarations.
    Ok(quote! {
        #display

        impl #impl_generics ::kynos::error::problem::IntoProblem
            for #name #ty_generics #where_clause
        {
            fn into_problem(self) -> ::kynos::Problem {
                #problem
            }

            fn statuses() -> &'static [::kynos::http::StatusCode] {
                // `StatusCode` has no const constructor, so the validated codes
                // are built once, on first use while the router is built.
                static STATUSES: ::std::sync::LazyLock<
                    ::std::vec::Vec<::kynos::http::StatusCode>
                > = ::std::sync::LazyLock::new(|| {
                    ::std::vec![
                        #(
                            ::kynos::http::StatusCode::from_u16(#statuses)
                                .expect("the derive checked this code")
                        ),*
                    ]
                });
                &STATUSES
            }
        }

        impl #impl_generics ::kynos::response::IntoResponse for #name #ty_generics #where_clause {
            fn into_response(self) -> ::kynos::http::Response {
                ::kynos::response::IntoResponse::into_response(
                    ::kynos::error::problem::IntoProblem::into_problem(self),
                )
            }
        }

        impl #impl_generics ::kynos::response::Responses for #name #ty_generics #where_clause {
            fn responses(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Responses {
                #responses
            }
        }

        // The same list as a `const`, so two interceptors claiming one status
        // is a compile error.
        impl #impl_generics ::kynos::response::ShortCircuit for #name #ty_generics #where_clause {
            const STATUSES: &'static [u16] = &[#(#statuses),*];
        }
    })
}

/// One way the type can fail: a pattern that matches it, and what it says.
struct Failure {
    /// The pattern `into_problem` matches this failure with, already carrying
    /// bindings for every published extension member.
    pattern: TokenStream2,

    /// The extension members, as the name each is published under and the
    /// binding the pattern gave it.
    extensions: Vec<(String, Ident)>,

    status: u16,
    type_uri: Option<String>,
    title: Option<String>,

    /// The prose a reader already wrote, used where no `title` was given.
    doc: Option<String>,
}

/// The `into_problem` body; `detail` is taken from `Display` before the
/// extension members are moved out.
fn into_problem(failures: &[Failure]) -> TokenStream2 {
    let arms = failures.iter().map(|failure| {
        let pattern = &failure.pattern;
        let status = failure.status;

        let with_type = failure
            .type_uri
            .as_ref()
            .map(|uri| quote!(problem.type_uri = ::std::borrow::Cow::Borrowed(#uri);));
        let with_title = failure
            .title
            .as_ref()
            .map(|title| quote!(problem.title = ::std::borrow::Cow::Borrowed(#title);));
        let with_extensions = failure
            .extensions
            .iter()
            .map(|(name, binding)| quote!(problem = problem.with_extension(#name, #binding);));

        quote! {
            #pattern => {
                let status = ::kynos::http::StatusCode::from_u16(#status)
                    .expect("the derive checked this code");
                // `new` titles it with the reason phrase, as RFC 9457 asks of
                // `about:blank`.
                let mut problem = ::kynos::Problem::new(status);
                #with_type
                #with_title
                problem.detail = ::core::option::Option::Some(detail);
                #(#with_extensions)*
                problem
            }
        }
    });

    quote! {
        let detail = ::std::string::ToString::to_string(&self);
        match self {
            #(#arms)*
        }
    }
}

/// The `responses` body: one response per distinct status, over the `Problem`
/// component, shaped by `kynos::__private::problem::response`.
fn responses(failures: &[Failure], statuses: &[u16]) -> TokenStream2 {
    let entries = statuses.iter().map(|status| {
        // Every failure declaring this status, in declaration order.
        let branches = failures
            .iter()
            .filter(|failure| failure.status == *status)
            .map(|failure| {
                let uri = optional(failure.type_uri.as_deref());
                let summary = optional(failure.title.as_deref().or(failure.doc.as_deref()));
                quote!((#uri, #summary))
            });

        quote! {
            responses = responses.with(
                #status,
                ::kynos::__private::problem::response(&schema, #status, &[#(#branches),*]),
            );
        }
    });

    quote! {
        let schema = registry.resolve::<::kynos::Problem>();
        let mut responses = ::kynos::openapi::Responses::new();
        #(#entries)*
        responses
    }
}

/// A string the declaration may not have given, as the `Option` the helper
/// reads it as.
fn optional(value: Option<&str>) -> TokenStream2 {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |value| quote!(::core::option::Option::Some(#value)),
    )
}

/// The statuses in declaration order, without repeats; two variants may share
/// a status.
fn distinct_statuses(failures: &[Failure]) -> Vec<u16> {
    let mut seen: Vec<u16> = Vec::new();
    for failure in failures {
        if !seen.contains(&failure.status) {
            seen.push(failure.status);
        }
    }
    seen
}

/// Every way the type can fail, read from the `#[problem(...)]` declarations.
fn failures(input: &DeriveInput) -> syn::Result<Vec<Failure>> {
    let base = parse_problem(&input.attrs, Position::Type)?.base;

    match &input.data {
        Data::Enum(data) => {
            // A status belongs on each variant, not the enum.
            if let Some(status) = parse_problem(&input.attrs, Position::Type)?.status {
                return Err(syn::Error::new(
                    status.1,
                    "a status belongs on each variant, because the point of an `ApiError` enum is \
                     that its variants fail differently. Move it to the variants",
                ));
            }

            data.variants
                .iter()
                .map(|variant| {
                    reject_unnamed_extensions(&variant.fields)?;

                    let args = parse_problem(&variant.attrs, Position::Variant)?;
                    let Some((status, _)) = args.status else {
                        return Err(syn::Error::new(
                            variant.ident.span(),
                            format!(
                                "variant `{}` does not say what status it produces; add \
                                 `#[problem(status = ...)]`",
                                variant.ident
                            ),
                        ));
                    };

                    let extensions = extensions(&variant.fields);
                    let bindings = extensions.iter().map(|(_, binding)| binding);
                    let name = &variant.ident;

                    Ok(Failure {
                        pattern: quote!(Self::#name { #(#bindings,)* .. }),
                        extensions,
                        status,
                        type_uri: type_uri(&args, base.as_deref(), &variant.ident),
                        title: args.title,
                        doc: doc_string(&variant.attrs),
                    })
                })
                .collect()
        }

        Data::Struct(data) => {
            reject_unnamed_extensions(&data.fields)?;

            let args = parse_problem(&input.attrs, Position::Type)?;
            let Some((status, _)) = args.status else {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "this error does not say what status it produces; add \
                     `#[problem(status = ...)]`, or use an enum when it can fail several ways",
                ));
            };

            let extensions = extensions(&data.fields);
            let bindings = extensions.iter().map(|(_, binding)| binding);

            Ok(vec![Failure {
                pattern: quote!(Self { #(#bindings,)* .. }),
                extensions,
                status,
                type_uri: type_uri(&args, base.as_deref(), &input.ident),
                title: args.title,
                doc: doc_string(&input.attrs),
            }])
        }

        Data::Union(_) => unreachable!("rejected above"),
    }
}

/// The URI identifying this failure's *type*, if the declaration gives one.
///
/// An explicit `type` wins, else `base` plus the name as a slug; with neither,
/// the problem keeps RFC 9457's `about:blank`.
fn type_uri(args: &ProblemArgs, base: Option<&str>, name: &Ident) -> Option<String> {
    if let Some(uri) = &args.type_uri {
        return Some(uri.clone());
    }
    base.map(|base| format!("{base}{}", kebab(&name.to_string())))
}

/// A Rust type or variant name as a URI slug.
fn kebab(name: &str) -> String {
    let mut slug = String::with_capacity(name.len() + 4);
    for (index, character) in name.char_indices() {
        if character.is_uppercase() && index != 0 {
            slug.push('-');
        }
        slug.extend(character.to_lowercase());
    }
    slug
}

/// The named fields marked `#[problem(extension)]`, as the name each is
/// published under and the identifier the pattern binds it to.
fn extensions(fields: &Fields) -> Vec<(String, Ident)> {
    let Fields::Named(named) = fields else {
        return Vec::new();
    };

    named
        .named
        .iter()
        .filter(|field| field.attrs.iter().any(is_extension))
        .filter_map(|field| field.ident.clone().map(|ident| (ident.to_string(), ident)))
        .collect()
}

/// Whether one attribute is `#[problem(extension)]`.
fn is_extension(attr: &Attribute) -> bool {
    if !attr.path().is_ident("problem") {
        return false;
    }
    let mut found = false;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("extension") {
            found = true;
            return Ok(());
        }
        skip_value(&meta)
    });
    found
}

/// Where a `#[problem(...)]` list is written, which decides its legal members.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Type,
    Variant,
}

/// What one item's `#[problem(...)]` lists said.
#[derive(Default)]
struct ProblemArgs {
    status: Option<(u16, Span)>,
    title: Option<String>,
    type_uri: Option<String>,
    base: Option<String>,
}

/// Reads one item's `#[problem(...)]` lists, validating every member.
fn parse_problem(attrs: &[Attribute], position: Position) -> syn::Result<ProblemArgs> {
    let mut args = ProblemArgs::default();

    for attr in attrs {
        if !attr.path().is_ident("problem") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let Some(key) = meta.path.get_ident() else {
                return Ok(());
            };

            match key.to_string().as_str() {
                "status" => {
                    let literal: LitInt = meta.value()?.parse()?;
                    let code: u16 = literal.base10_parse()?;
                    if !STATUS_RANGE.contains(&code) {
                        return Err(syn::Error::new(
                            literal.span(),
                            format!(
                                "a problem detail describes a failure, so its status is between \
                                 {} and {}; `{code}` is not",
                                STATUS_RANGE.start(),
                                STATUS_RANGE.end()
                            ),
                        ));
                    }
                    if args.status.is_some() {
                        return Err(syn::Error::new(
                            literal.span(),
                            "this already declares a status, and a response has one",
                        ));
                    }
                    args.status = Some((code, literal.span()));
                }
                "title" => args.title = Some(meta.value()?.parse::<LitStr>()?.value()),
                "type" => args.type_uri = Some(meta.value()?.parse::<LitStr>()?.value()),
                "base" if position == Position::Type => {
                    args.base = Some(meta.value()?.parse::<LitStr>()?.value());
                }
                "base" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`base` is the prefix every type URI shares, so it belongs on the type \
                         rather than on one variant",
                    ));
                }
                "extension" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`extension` marks a field to publish, so it belongs on a field",
                    ));
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("`{other}` is not part of the `#[problem(...)]` grammar"),
                    ));
                }
            }
            Ok(())
        })?;
    }

    Ok(args)
}

/// Refuses `#[problem(extension)]` on an unnamed field, which has no member
/// name.
fn reject_unnamed_extensions(fields: &Fields) -> syn::Result<()> {
    let Fields::Unnamed(unnamed) = fields else {
        return Ok(());
    };

    for field in &unnamed.unnamed {
        for attr in &field.attrs {
            if !attr.path().is_ident("problem") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("extension") {
                    return Err(syn::Error::new(
                        attr.span(),
                        "an extension member is published under its field's name, and this field \
                         has none. Give the variant named fields",
                    ));
                }
                skip_value(&meta)
            })?;
        }
    }

    Ok(())
}
