//! `#[derive(SecurityScheme)]`.
//!
//! The kind is nested so `name` (the component key) and an API key's field
//! `name` do not collide.
//!
//! ```text
//! #[security( <kind> )]                     // required, exactly once
//! #[security( <option> [, <option>]* )]     // optional, repeatable
//!
//! kind   := bearer | bearer(format = "JWT")
//!         | basic
//!         | http(scheme = "<RFC 9110 auth-scheme token>" [, format = "..."])
//!         | api_key(in = "header" | "query" | "cookie", name = "<field>")
//!         | mutual_tls
//!         | openid_connect(url = "<discovery URL>")
//!         | oauth2( <flow>+ [, metadata_url = "..."] )
//!
//! flow   := implicit( authorization_url = "..", [refresh_url = ".."], [scopes(..)] )
//!         | password( token_url = "..", [refresh_url = ".."], [scopes(..)] )
//!         | client_credentials( token_url = "..", [refresh_url = ".."], [scopes(..)] )
//!         | authorization_code( authorization_url = "..", token_url = "..",
//!                               [refresh_url = ".."], [scopes(..)] )
//!         | device_authorization( device_authorization_url = "..", token_url = "..",
//!                                 [refresh_url = ".."], [scopes(..)] )   // 3.2
//!
//! option := name = "<ComponentName>" | credential = <Type> | scope = <Type>
//!         | description = "<CommonMark>" | challenge = "<WWW-Authenticate>"
//!         | scopes(<expr>, ..) | deprecated | carrier = manual | carrier = optional
//!         | unavailable | forbids = <bool>
//! ```
//!
//! Each kind and each flow takes only the options its row lists, each once; a
//! flow's URLs are the ones the OAuth Flow Object applies to it. An option
//! elsewhere, a qualified key, and a key written twice, across lists too, are
//! refused rather than skipped or overwritten.
//!
//! The kind writes both `describe` and `Carries`, so the documented and the
//! enforced carrier agree; `carrier = manual` suppresses `Carries`, and
//! `carrier = optional` presents `Option<_>`, so an absent credential reaches
//! the authenticator rather than answering `Ok(None)`.
//!
//! A flow's `scopes` takes `"a"` or `"a" = "Read a"`, the strings an
//! authorization server publishes. The scheme-level `scopes(..)` is what an
//! operation demands, and takes values of `scope = Type`, which is
//! `Unscoped` when omitted; a value of any other type fails to typecheck.

mod args;
mod flow;

use quote::quote;
use syn::{DeriveInput, LitStr, Type, parse_quote};

use crate::derive::common::{optional, unit_struct};
use crate::doc::doc_string;
use args::{Carrier, KeyLocation, Kind, Nested, SchemeArgs, parse_args};
use flow::build_flow;

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // A scheme is a marker; the credential type carries the data.
    unit_struct(input, "SecurityScheme", "names a way of authenticating")?;

    let args = parse_args(input)?;
    let Some(kind) = args.kind else {
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
            fn scopes() -> &'static [Self::Scope] {
                &[#(#scopes),*]
            }
        }
    });
    let scope: Type = args
        .scope
        .unwrap_or_else(|| parse_quote!(::kynos::security::Unscoped));

    let declared = args
        .name
        .unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));
    let credential = args
        .credential
        .unwrap_or_else(|| parse_quote!(::std::string::String));

    let challenge = args.challenge.map_or_else(
        || default_challenge(kind, &args.nested),
        |value| quote!(::core::option::Option::Some(#value)),
    );
    let insufficient_scope = default_insufficient_scope_challenge(kind);
    let unavailable = args.unavailable.then(|| {
        quote!(
            const MAY_BE_UNAVAILABLE: bool = true;
        )
    });
    let forbids = args.forbids.map(|f| quote!(const FORBIDS: bool = #f;));

    let carrier = (args.carrier != Some(Carrier::Manual)).then(|| {
        let (mut presented, mut read) = carrier_of(&args.nested, kind);
        // A malformed credential stays an `Err`; only absence becomes a value.
        if args.carrier == Some(Carrier::Optional) {
            presented = quote!(::core::option::Option<#presented>);
            read = quote!(#read.map(::core::option::Option::Some));
        }
        quote! {
            impl #impl_generics ::kynos::security::carrier::Carries
                for #name #ty_generics #where_clause
            {
                type Presented = #presented;

                fn present(
                    parts: &::kynos::http::Parts,
                ) -> ::core::result::Result<
                    ::core::option::Option<Self::Presented>,
                    ::kynos::error::rejection::AuthRejection,
                > {
                    #read
                }
            }
        }
    });

    Ok(quote! {
        impl #impl_generics ::kynos::security::SecurityScheme
            for #name #ty_generics #where_clause
        {
            const NAME: &'static str = #declared;
            #unavailable #forbids

            type Credential = #credential;

            type Scope = #scope;

            fn describe() -> ::kynos::openapi::SecurityScheme {
                #description
            }

            #scopes

            fn challenge() -> ::core::option::Option<&'static str> {
                #challenge
            }

            fn insufficient_scope_challenge() -> ::core::option::Option<&'static str> {
                #insufficient_scope
            }
        }

        #carrier
    })
}

/// The text an option was given, or the empty string where it was not.
fn text(value: Option<&LitStr>) -> String {
    value.map_or_else(String::new, LitStr::value)
}

/// Where this kind's credential travels, and the type it reads back as; read
/// from the same `Nested` as `of_kind`.
fn carrier_of(nested: &Nested, kind: Kind) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let carrier = quote!(::kynos::security::carrier);

    match kind {
        Kind::Basic => (
            quote!(#carrier::Credentials),
            quote!(#carrier::basic(parts)),
        ),

        Kind::Http => {
            let scheme = text(nested.scheme.as_ref());
            (
                quote!(#carrier::SchemeCredentials),
                quote!(#carrier::http_scheme(parts, #scheme)),
            )
        }

        Kind::ApiKey(location) => {
            let field = text(nested.field.as_ref());
            let location = match location {
                KeyLocation::Header => quote!(#carrier::KeyLocation::Header),
                KeyLocation::Query => quote!(#carrier::KeyLocation::Query),
                KeyLocation::Cookie => quote!(#carrier::KeyLocation::Cookie),
            };
            (
                quote!(#carrier::ApiKey),
                quote!(#carrier::api_key(parts, #location, #field)),
            )
        }

        Kind::MutualTls => (
            quote!(#carrier::PeerCertificates),
            quote!(#carrier::peer_certificates(parts)),
        ),

        // An RFC 6750 bearer token.
        Kind::Bearer | Kind::OAuth2 | Kind::OpenIdConnect => (
            quote!(#carrier::BearerToken),
            quote!(#carrier::bearer(parts)),
        ),
    }
}

/// The `describe` body: the kind, then the prose and the deprecation every kind
/// shares, the latter applied once by matching.
fn describe(args: &SchemeArgs, kind: Kind, input: &DeriveInput) -> proc_macro2::TokenStream {
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
                            <::std::string::String as ::core::convert::From<_>>::from(#text),
                        );
                    }
                    // Unreachable today; `SecurityScheme` is `#[non_exhaustive]`.
                    _ => {}
                }
            }
        });

    // The `cfg!` keeps the arm out of a 3.1 expansion, where the field does not
    // exist; an emitted `#[cfg]` would read the application's features instead.
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
                _ => {}
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

/// One scheme of the declared kind, with nothing shared filled in yet; built
/// through the model's constructors so no 3.2-only field is named here.
fn of_kind(args: &SchemeArgs, kind: Kind) -> proc_macro2::TokenStream {
    let owned = |value: Option<&LitStr>| {
        let from = quote!(<::std::string::String as ::core::convert::From<_>>::from);
        optional(value.map(|value| quote!(#from(#value))))
    };

    match kind {
        Kind::Basic => quote!(::kynos::openapi::SecurityScheme::basic()),

        Kind::Http => {
            let format = owned(args.nested.format.as_ref());
            let scheme = text(args.nested.scheme.as_ref());
            quote! {
                {
                    let mut scheme = ::kynos::openapi::SecurityScheme::bearer(#format);
                    if let ::kynos::openapi::SecurityScheme::Http { scheme: name, .. } =
                        &mut scheme
                    {
                        *name = <::std::string::String as ::core::convert::From<_>>::from(#scheme);
                    }
                    scheme
                }
            }
        }

        Kind::ApiKey(location) => {
            let field = text(args.nested.field.as_ref());
            match location {
                KeyLocation::Header => {
                    quote!(::kynos::openapi::SecurityScheme::api_key_header(#field))
                }
                KeyLocation::Query => {
                    quote!(::kynos::openapi::SecurityScheme::api_key_query(#field))
                }
                KeyLocation::Cookie => {
                    quote!(::kynos::openapi::SecurityScheme::api_key_cookie(#field))
                }
            }
        }

        Kind::MutualTls => quote!(::kynos::openapi::SecurityScheme::mutual_tls()),

        Kind::OpenIdConnect => {
            let url = text(args.nested.url.as_ref());
            quote!(::kynos::openapi::SecurityScheme::open_id_connect(#url))
        }

        Kind::OAuth2 => {
            let flows = args.nested.flows.iter().map(|(name, row, flow)| {
                let builder = syn::Ident::new(row.builder, name.span());
                let built = build_flow(flow);
                quote!(.#builder(#built))
            });

            // Through the model's builder, chained onto the `oauth2` scheme.
            let metadata = args
                .nested
                .metadata_url
                .as_ref()
                .filter(|_| cfg!(feature = "openapi32"))
                .map(|url| quote!(.with_oauth2_metadata_url(#url)));

            quote! {
                ::kynos::openapi::SecurityScheme::oauth2(
                    <::kynos::openapi::OAuthFlows as ::core::default::Default>::default()
                        #(#flows)*
                )
                #metadata
            }
        }

        Kind::Bearer => {
            let format = owned(args.nested.format.as_ref());
            quote!(::kynos::openapi::SecurityScheme::bearer(#format))
        }
    }
}

/// The challenge a kind sends without being told.
///
/// `bearer`, `oauth2` and `openid_connect` answer `Bearer` (RFC 6750 section
/// 3); `basic` answers what `kynos::security::schemes::Basic` does, naming
/// its default realm and `charset="UTF-8"` (RFC 7617 section 2), so the two
/// cannot drift; `challenge = "..."` names another realm. RFC 9110 section
/// 15.5.2 requires a challenge on every 401, so `api_key` answers `ApiKey` and
/// `mutual_tls` what `kynos::security::schemes::MutualTls` does, each an
/// unregistered `auth-scheme` token naming the OpenAPI type. `http` answers
/// its own `scheme` token bare, since a challenge may carry no parameters; the
/// ones its scheme needs, such as Digest's `realm` and `nonce`, are the
/// application's to add through `challenge = "..."`.
fn default_challenge(kind: Kind, nested: &Nested) -> proc_macro2::TokenStream {
    match kind {
        Kind::Bearer | Kind::OAuth2 | Kind::OpenIdConnect => {
            quote!(::core::option::Option::Some("Bearer"))
        }
        Kind::Http => {
            let scheme = text(nested.scheme.as_ref());
            quote!(::core::option::Option::Some(#scheme))
        }
        Kind::Basic => quote! {
            <::kynos::security::schemes::Basic as ::kynos::security::SecurityScheme>::challenge()
        },
        Kind::ApiKey(_) => quote!(::core::option::Option::Some("ApiKey")),
        Kind::MutualTls => quote! {
            <::kynos::security::schemes::MutualTls as ::kynos::security::SecurityScheme>::challenge()
        },
    }
}

/// The challenge a kind sends on a 403 refusing a scope.
///
/// `bearer`, `oauth2` and `openid_connect` answer what
/// `kynos::security::schemes::Bearer` does, `Bearer error="insufficient_scope"`
/// (RFC 6750 section 3.1), so the two cannot drift. The other kinds have no
/// registered error to send. `challenge = "..."` replaces only the 401's.
fn default_insufficient_scope_challenge(kind: Kind) -> proc_macro2::TokenStream {
    match kind {
        Kind::Bearer | Kind::OAuth2 | Kind::OpenIdConnect => quote! {
            <::kynos::security::schemes::Bearer as ::kynos::security::SecurityScheme>::insufficient_scope_challenge()
        },
        Kind::Basic | Kind::Http | Kind::ApiKey(_) | Kind::MutualTls => {
            quote!(::core::option::Option::None)
        }
    }
}
