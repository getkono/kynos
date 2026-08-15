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
use syn::{DeriveInput, Ident, LitStr, Token, Type, parse_macro_input, parse_quote};

use crate::derive::common::{doc_string, skip_value, unit_struct};

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
    description: Option<LitStr>,
    deprecated: bool,
    scopes: Vec<LitStr>,
    nested: Nested,
}

/// The options written inside a kind, whichever kind it was.
///
/// One flat set rather than one per kind, because the kinds are already
/// distinguished by [`SchemeArgs::kind`] and each key means the same thing
/// wherever it is legal: `format` is a bearer format, `scheme` an RFC 7235
/// name, `url` a discovery URL.
#[derive(Default)]
struct Nested {
    location: Option<LitStr>,
    field: Option<LitStr>,
    scheme: Option<LitStr>,
    format: Option<LitStr>,
    url: Option<LitStr>,
    metadata_url: Option<LitStr>,
}

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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

    let description = describe(&args, kind, input);
    let scopes = (!args.scopes.is_empty()).then(|| {
        let scopes = &args.scopes;
        quote! {
            fn scopes() -> &'static [&'static str] {
                &[#(#scopes),*]
            }
        }
    });

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
                #description
            }

            #scopes

            fn challenge() -> ::core::option::Option<&'static str> {
                #challenge
            }
        }
    })
}

/// The `describe` body: the kind, then the prose and the deprecation every kind
/// shares.
///
/// The shared half is applied by matching rather than by five constructions,
/// because `description` means the same thing in every variant and writing it
/// once is what stops one kind quietly losing it.
fn describe(args: &SchemeArgs, kind: &Ident, input: &DeriveInput) -> proc_macro2::TokenStream {
    let built = of_kind(args, kind);

    let described = args
        .description
        .as_ref()
        .map(LitStr::value)
        .or_else(|| doc_string(&input.attrs))
        .map(|text| {
            quote! {
                match &mut scheme {
                    ::kynos::openapi::SecurityScheme::ApiKey { description, .. }
                    | ::kynos::openapi::SecurityScheme::Http { description, .. }
                    | ::kynos::openapi::SecurityScheme::MutualTls { description, .. }
                    | ::kynos::openapi::SecurityScheme::OAuth2 { description, .. }
                    | ::kynos::openapi::SecurityScheme::OpenIdConnect { description, .. } => {
                        *description = ::core::option::Option::Some(
                            ::std::string::String::from(#text),
                        );
                    }
                }
            }
        });

    // 3.2 introduced `deprecated`, and a build without it has no field to put
    // the answer in. Decided here rather than in the expansion, because a
    // `#[cfg]` emitted into an application's crate would read that crate's
    // features rather than the document model's.
    let deprecated = (args.deprecated && cfg!(feature = "openapi32")).then(|| {
        quote! {
            match &mut scheme {
                ::kynos::openapi::SecurityScheme::ApiKey { deprecated, .. }
                | ::kynos::openapi::SecurityScheme::Http { deprecated, .. }
                | ::kynos::openapi::SecurityScheme::MutualTls { deprecated, .. }
                | ::kynos::openapi::SecurityScheme::OAuth2 { deprecated, .. }
                | ::kynos::openapi::SecurityScheme::OpenIdConnect { deprecated, .. } => {
                    *deprecated = ::core::option::Option::Some(true);
                }
            }
        }
    });

    quote! {
        let mut scheme = #built;
        #described
        #deprecated
        scheme
    }
}

/// One scheme of the declared kind, with nothing shared filled in yet.
///
/// Built through the model's own constructors wherever one exists, so that a
/// field the specification only has from 3.2 onward is never named here.
fn of_kind(args: &SchemeArgs, kind: &Ident) -> proc_macro2::TokenStream {
    let optional = |value: Option<&LitStr>| {
        value.map_or_else(
            || quote!(::core::option::Option::None),
            |value| quote!(::core::option::Option::Some(::std::string::String::from(#value))),
        )
    };
    let text = |value: Option<&LitStr>| value.map_or_else(String::new, LitStr::value);

    match kind.to_string().as_str() {
        "basic" => quote!(::kynos::openapi::SecurityScheme::basic()),

        "http" => {
            let format = optional(args.nested.format.as_ref());
            let scheme = text(args.nested.scheme.as_ref());
            quote! {
                {
                    let mut scheme = ::kynos::openapi::SecurityScheme::bearer(#format);
                    if let ::kynos::openapi::SecurityScheme::Http { scheme: name, .. } =
                        &mut scheme
                    {
                        *name = ::std::string::String::from(#scheme);
                    }
                    scheme
                }
            }
        }

        "api_key" => {
            let field = text(args.nested.field.as_ref());
            let location = match args.nested.location.as_ref().map(LitStr::value).as_deref() {
                Some("query") => quote!(::kynos::openapi::ParameterIn::Query),
                Some("cookie") => quote!(::kynos::openapi::ParameterIn::Cookie),
                _ => quote!(::kynos::openapi::ParameterIn::Header),
            };
            quote! {
                {
                    let mut scheme = ::kynos::openapi::SecurityScheme::api_key_header(#field);
                    if let ::kynos::openapi::SecurityScheme::ApiKey { location: carried, .. } =
                        &mut scheme
                    {
                        *carried = #location;
                    }
                    scheme
                }
            }
        }

        "mutual_tls" => quote!(::kynos::openapi::SecurityScheme::mutual_tls()),

        "openid_connect" => {
            let url = text(args.nested.url.as_ref());
            let deprecated = gated_deprecated();
            quote! {
                ::kynos::openapi::SecurityScheme::OpenIdConnect {
                    open_id_connect_url: ::std::string::String::from(#url),
                    description: ::core::option::Option::None,
                    #deprecated
                    extensions: ::kynos::openapi::Extensions::new(),
                }
            }
        }

        "oauth2" => {
            let deprecated = gated_deprecated();
            let metadata = args
                .nested
                .metadata_url
                .as_ref()
                .filter(|_| cfg!(feature = "openapi32"))
                .map(|url| {
                    quote!(oauth2_metadata_url: ::core::option::Option::Some(
                        ::std::string::String::from(#url)
                    ),)
                })
                .or_else(|| {
                    cfg!(feature = "openapi32")
                        .then(|| quote!(oauth2_metadata_url: ::core::option::Option::None,))
                });
            quote! {
                ::kynos::openapi::SecurityScheme::OAuth2 {
                    flows: ::std::boxed::Box::new(
                        ::kynos::openapi::OAuthFlows::default(),
                    ),
                    #metadata
                    description: ::core::option::Option::None,
                    #deprecated
                    extensions: ::kynos::openapi::Extensions::new(),
                }
            }
        }

        // `bearer`, and the fallback for a kind `is_kind` admits and this match
        // has not learned: an HTTP scheme is the one every other kind degrades
        // to safely, since it claims nothing beyond a credential in
        // `Authorization`.
        _ => {
            let format = optional(args.nested.format.as_ref());
            quote!(::kynos::openapi::SecurityScheme::bearer(#format))
        }
    }
}

/// The `deprecated` field, where the document model carries one.
fn gated_deprecated() -> Option<proc_macro2::TokenStream> {
    cfg!(feature = "openapi32").then(|| quote!(deprecated: ::core::option::Option::None,))
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
                "description" => args.description = Some(meta.value()?.parse()?),
                "deprecated" => args.deprecated = true,
                "scopes" => {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    args.scopes.extend(
                        content
                            .parse_terminated(<LitStr as syn::parse::Parse>::parse, Token![,])?,
                    );
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
                    check_kind(key, &meta, &mut args.nested)?;
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

/// Reads the options nested inside one kind, and checks the ones that are
/// checkable.
///
/// One flat reader for every kind, because each key means the same thing
/// wherever it is legal. Only `api_key` is *checked* in full: its rejections
/// are the ones a user hits, and each has its span here and nowhere else.
fn check_kind(
    kind: &Ident,
    meta: &syn::meta::ParseNestedMeta<'_>,
    nested: &mut Nested,
) -> syn::Result<()> {
    if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|option| {
            let Some(key) = option.path.get_ident() else {
                return skip_value(&option);
            };
            match key.to_string().as_str() {
                "in" => nested.location = Some(option.value()?.parse()?),
                "name" => nested.field = Some(option.value()?.parse()?),
                "scheme" => nested.scheme = Some(option.value()?.parse()?),
                "format" => nested.format = Some(option.value()?.parse()?),
                "url" => nested.url = Some(option.value()?.parse()?),
                "metadata_url" => nested.metadata_url = Some(option.value()?.parse()?),
                _ => skip_value(&option)?,
            }
            Ok(())
        })?;
    } else {
        // A kind written bare -- `bearer`, `basic`, `mutual_tls` -- has no list
        // to read.
        skip_value(meta)?;
    }

    if kind != "api_key" {
        return Ok(());
    }

    let Some(location) = nested.location.clone() else {
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

    let Some(field) = nested.field.clone() else {
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
