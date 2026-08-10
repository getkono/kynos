//! `#[derive(SecurityScheme)]`.
//!
//! The grammar nests the scheme *kind* so that `name` is unambiguous: the
//! component key a document registers the scheme under and the field name an
//! API key travels in are different things that would otherwise share a word.
//!
//! ```text
//! #[security( <kind> )]                     // required, exactly once
//! #[security( <option> [, <option>]* )]     // optional, repeatable
//!
//! kind   := bearer | bearer(format = "JWT")
//!         | basic
//!         | http(scheme = "<RFC 7235 token>" [, format = "..."])
//!         | api_key(in = "header" | "query" | "cookie", name = "<field>")
//!         | mutual_tls
//!         | openid_connect(url = "<discovery URL>")
//!         | oauth2( <flow>+ [, metadata_url = "..."] )
//!
//! option := name = "<ComponentName>" | credential = <Type>
//!         | description = "<CommonMark>" | challenge = "<WWW-Authenticate>"
//!         | scopes("a", "b") | deprecated
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident, LitStr, Type, parse_macro_input, parse_quote};

use crate::derive::common::unit_struct;

/// Locations an API key may travel in.
///
/// `path` is absent because a key in the path is part of the URL rather than a
/// credential, and `querystring` because 3.2's whole-query parameter describes
/// a payload rather than a named field.
const API_KEY_LOCATIONS: &[&str] = &["header", "query", "cookie"];

/// Header names a parameter definition may not claim.
///
/// The specification says such a definition shall be ignored, so an API key
/// declared under one would be a claim no consumer honours.
const RESERVED_HEADERS: &[&str] = &["authorization", "accept", "content-type"];

/// What the attribute said, before it becomes a description.
#[derive(Default)]
struct SchemeArgs {
    kind: Option<Ident>,
    name: Option<LitStr>,
    credential: Option<Type>,
    challenge: Option<LitStr>,
}

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // A scheme is a marker: it names a way of authenticating and carries no
    // data. What carries data is the credential, named by the associated type.
    unit_struct(input, "SecurityScheme", "names a way of authenticating")?;

    let args = parse_args(input)?;
    let Some(kind) = &args.kind else {
        return Err(syn::Error::new(
            input.ident.span(),
            "a security scheme must say what kind it is: `#[security(bearer)]`, \
             `#[security(basic)]`, `#[security(api_key(in = \"header\", name = \"X-Api-Key\"))]`, \
             `#[security(mutual_tls)]`, `#[security(openid_connect(url = \"...\"))]` or \
             `#[security(oauth2(...))]`",
        ));
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let declared = args
        .name
        .unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));
    let credential: Type = args.credential.unwrap_or_else(|| parse_quote!(String));

    let challenge = args.challenge.map_or_else(
        || default_challenge(kind),
        |value| quote!(::core::option::Option::Some(#value)),
    );

    Ok(quote! {
        impl #impl_generics ::kynos::security::SecurityScheme
            for #name #ty_generics #where_clause
        {
            const NAME: &'static str = #declared;

            type Credential = #credential;

            fn describe() -> ::kynos::openapi::SecurityScheme {
                ::core::todo!()
            }

            fn challenge() -> ::core::option::Option<&'static str> {
                #challenge
            }
        }
    })
}

/// The challenge an HTTP authentication scheme sends without being told.
fn default_challenge(kind: &Ident) -> proc_macro2::TokenStream {
    match kind.to_string().as_str() {
        "bearer" => quote!(::core::option::Option::Some("Bearer")),
        "basic" => quote!(::core::option::Option::Some("Basic")),
        _ => quote!(::core::option::Option::None),
    }
}

fn parse_args(input: &DeriveInput) -> syn::Result<SchemeArgs> {
    let mut args = SchemeArgs::default();

    for attr in &input.attrs {
        if !attr.path().is_ident("security") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let Some(key) = meta.path.get_ident() else {
                return Ok(());
            };

            match key.to_string().as_str() {
                "name" => args.name = Some(meta.value()?.parse()?),
                "credential" => args.credential = Some(meta.value()?.parse()?),
                "challenge" => args.challenge = Some(meta.value()?.parse()?),
                // Read when `describe` is implemented; parsing past them keeps
                // the attribute usable now and the grammar stable.
                "description" | "scopes" | "deprecated" => {
                    let _ = meta.input.parse::<proc_macro2::TokenStream>();
                }
                kind if is_kind(kind) => {
                    if let Some(existing) = &args.kind {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "a security scheme has exactly one kind, and this one is already \
                                 `{existing}`"
                            ),
                        ));
                    }
                    check_kind(key, &meta)?;
                    args.kind = Some(key.clone());
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("`{other}` is not part of the `#[security(...)]` grammar"),
                    ));
                }
            }
            Ok(())
        })?;
    }

    Ok(args)
}

fn is_kind(name: &str) -> bool {
    matches!(
        name,
        "bearer" | "basic" | "http" | "api_key" | "mutual_tls" | "openid_connect" | "oauth2"
    )
}

/// Checks the options nested inside one kind.
///
/// Only `api_key` is checked in full today: its two rejections are the ones a
/// user hits, and both have their span here and nowhere else.
fn check_kind(kind: &Ident, meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if kind != "api_key" {
        // The other kinds' options are consumed when `describe` lands.
        let _ = meta.input.parse::<proc_macro2::TokenStream>();
        return Ok(());
    }

    let mut location = None;
    let mut field = None;
    meta.parse_nested_meta(|nested| {
        if nested.path.is_ident("in") {
            location = Some(nested.value()?.parse::<LitStr>()?);
        } else if nested.path.is_ident("name") {
            field = Some(nested.value()?.parse::<LitStr>()?);
        } else {
            let _ = nested.input.parse::<proc_macro2::TokenStream>();
        }
        Ok(())
    })?;

    let Some(location) = location else {
        return Err(syn::Error::new(
            kind.span(),
            "an API key must say where it travels: `in = \"header\"`, `\"query\"` or `\"cookie\"`",
        ));
    };
    if !API_KEY_LOCATIONS.contains(&location.value().as_str()) {
        return Err(syn::Error::new(
            location.span(),
            format!(
                "an API key travels in a header, a query parameter or a cookie, not `{}`",
                location.value()
            ),
        ));
    }

    let Some(field) = field else {
        return Err(syn::Error::new(
            kind.span(),
            "an API key must say which field carries it: `name = \"X-Api-Key\"`",
        ));
    };
    // HTTP field names are case-insensitive, so the check must be too.
    if location.value() == "header"
        && RESERVED_HEADERS.contains(&field.value().to_ascii_lowercase().as_str())
    {
        return Err(syn::Error::new(
            field.span(),
            format!(
                "`{}` must not be declared as a parameter: the specification says such a \
                 definition is ignored. For credentials use `http(scheme = \"...\")`, `bearer` or \
                 `basic`; for content negotiation, return `Negotiated<T>`",
                field.value()
            ),
        ));
    }

    Ok(())
}
