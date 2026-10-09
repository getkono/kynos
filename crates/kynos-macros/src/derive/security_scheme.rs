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
//!         | http(scheme = "<RFC 7235 token>" [, format = "..."])
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
//! option := name = "<ComponentName>" | credential = <Type>
//!         | description = "<CommonMark>" | challenge = "<WWW-Authenticate>"
//!         | scopes("a", "b") | deprecated | carrier = manual
//! ```
//!
//! The kind writes both `describe` and `Carries`, so the documented and the
//! enforced carrier agree; `carrier = manual` suppresses `Carries`.
//!
//! A flow's `scopes` takes `"a"` or `"a" = "Read a"`; the scheme-level
//! `scopes(..)` is what an operation demands, and takes names only.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident, LitStr, Token, Type, parse_macro_input, parse_quote};

use crate::derive::common::{doc_string, non_token_message, skip_value, unit_struct};

/// Locations an API key may travel in; not `path` or 3.2's `querystring`.
const API_KEY_LOCATIONS: &[&str] = &["header", "query", "cookie"];

/// Header names a parameter definition may not claim; the OpenAPI Parameter
/// Object says such a definition shall be ignored.
const RESERVED_HEADERS: &[&str] = &["authorization", "accept", "content-type"];

/// Every OAuth 2.0 flow, and the URLs its own grant cannot work without.
///
/// RFC 6749 sections 4.1 to 4.4 fix the first four; RFC 8628 fixes the fifth,
/// which OpenAPI 3.2 added.
const FLOWS: &[Flow] = &[
    Flow {
        name: "implicit",
        builder: "with_implicit",
        required: &["authorization_url"],
        since_three_two: false,
    },
    Flow {
        name: "password",
        builder: "with_password",
        required: &["token_url"],
        since_three_two: false,
    },
    Flow {
        name: "client_credentials",
        builder: "with_client_credentials",
        required: &["token_url"],
        since_three_two: false,
    },
    Flow {
        name: "authorization_code",
        builder: "with_authorization_code",
        required: &["authorization_url", "token_url"],
        since_three_two: false,
    },
    Flow {
        name: "device_authorization",
        builder: "with_device_authorization",
        required: &["device_authorization_url", "token_url"],
        since_three_two: true,
    },
];

/// One row of [`FLOWS`].
struct Flow {
    /// How the flow is spelled in the attribute.
    name: &'static str,
    /// The `OAuthFlows` builder that attaches it.
    builder: &'static str,
    /// The URL keys this grant cannot work without.
    required: &'static [&'static str],
    /// Whether only OpenAPI 3.2 can express it.
    since_three_two: bool,
}

/// The row `name` names.
fn flow_named(name: &str) -> Option<&'static Flow> {
    FLOWS.iter().find(|flow| flow.name == name)
}

/// What one declared flow said.
#[derive(Default)]
struct FlowArgs {
    authorization_url: Option<LitStr>,
    token_url: Option<LitStr>,
    device_authorization_url: Option<LitStr>,
    refresh_url: Option<LitStr>,
    scopes: Vec<(LitStr, Option<LitStr>)>,
}

impl FlowArgs {
    /// The value of one URL key, by the name [`Flow::required`] uses.
    fn url(&self, key: &str) -> Option<&LitStr> {
        match key {
            "authorization_url" => self.authorization_url.as_ref(),
            "token_url" => self.token_url.as_ref(),
            "device_authorization_url" => self.device_authorization_url.as_ref(),
            _ => None,
        }
    }
}

/// What the attribute said, before it becomes a description.
#[derive(Default)]
struct SchemeArgs {
    kind: Option<Ident>,
    name: Option<LitStr>,
    credential: Option<Type>,
    challenge: Option<LitStr>,
    description: Option<LitStr>,
    deprecated: bool,
    /// Whether the carrier is the application's to write.
    manual_carrier: bool,
    scopes: Vec<LitStr>,
    nested: Nested,
}

/// The options written inside a kind, whichever kind it was; one flat set,
/// since each key means the same thing wherever it is legal.
#[derive(Default)]
struct Nested {
    location: Option<LitStr>,
    field: Option<LitStr>,
    scheme: Option<LitStr>,
    format: Option<LitStr>,
    url: Option<LitStr>,
    metadata_url: Option<LitStr>,
    /// The OAuth 2.0 flows declared, in order; a `Vec` so a repeat is caught.
    flows: Vec<(Ident, FlowArgs)>,
}

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    // A scheme is a marker; the credential type carries the data.
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

    let carrier = (!args.manual_carrier).then(|| {
        let (presented, read) = carrier_of(&args.nested, kind);
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

            type Credential = #credential;

            fn describe() -> ::kynos::openapi::SecurityScheme {
                #description
            }

            #scopes

            fn challenge() -> ::core::option::Option<&'static str> {
                #challenge
            }
        }

        #carrier
    })
}

/// Where this kind's credential travels, and the type it reads back as; read
/// from the same `Nested` as `of_kind`.
fn carrier_of(
    nested: &Nested,
    kind: &Ident,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let carrier = quote!(::kynos::security::carrier);
    let text = |value: Option<&LitStr>| value.map_or_else(String::new, LitStr::value);

    match kind.to_string().as_str() {
        "basic" => (
            quote!(#carrier::Credentials),
            quote!(#carrier::basic(parts)),
        ),

        "http" => {
            let scheme = text(nested.scheme.as_ref());
            (
                quote!(#carrier::SchemeCredentials),
                quote!(#carrier::http_scheme(parts, #scheme)),
            )
        }

        "api_key" => {
            let field = text(nested.field.as_ref());
            let location = match nested.location.as_ref().map(LitStr::value).as_deref() {
                Some("query") => quote!(#carrier::KeyLocation::Query),
                Some("cookie") => quote!(#carrier::KeyLocation::Cookie),
                _ => quote!(#carrier::KeyLocation::Header),
            };
            (
                quote!(#carrier::ApiKey),
                quote!(#carrier::api_key(parts, #location, #field)),
            )
        }

        "mutual_tls" => (
            quote!(#carrier::PeerCertificates),
            quote!(#carrier::peer_certificates(parts)),
        ),

        // `bearer`, `oauth2` and `openid_connect`: an RFC 6750 bearer token.
        _ => (
            quote!(#carrier::BearerToken),
            quote!(#carrier::bearer(parts)),
        ),
    }
}

/// The `describe` body: the kind, then the prose and the deprecation every kind
/// shares, the latter applied once by matching.
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
            match args.nested.location.as_ref().map(LitStr::value).as_deref() {
                Some("query") => quote!(::kynos::openapi::SecurityScheme::api_key_query(#field)),
                Some("cookie") => quote!(::kynos::openapi::SecurityScheme::api_key_cookie(#field)),
                _ => quote!(::kynos::openapi::SecurityScheme::api_key_header(#field)),
            }
        }

        "mutual_tls" => quote!(::kynos::openapi::SecurityScheme::mutual_tls()),

        "openid_connect" => {
            let url = text(args.nested.url.as_ref());
            quote!(::kynos::openapi::SecurityScheme::open_id_connect(#url))
        }

        "oauth2" => {
            let flows = args.nested.flows.iter().map(|(name, flow)| {
                let builder = syn::Ident::new(
                    flow_named(&name.to_string())
                        .expect("`check_kind` refuses a flow this table does not name")
                        .builder,
                    name.span(),
                );
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
                    ::kynos::openapi::OAuthFlows::default()
                        #(#flows)*
                )
                #metadata
            }
        }

        // `bearer`, and the safe fallback for any kind this match has not learned.
        _ => {
            let format = optional(args.nested.format.as_ref());
            quote!(::kynos::openapi::SecurityScheme::bearer(#format))
        }
    }
}

/// One `OAuthFlow`, built through the model's own builders.
fn build_flow(flow: &FlowArgs) -> proc_macro2::TokenStream {
    let scopes = flow.scopes.iter().map(|(name, described)| {
        // An undescribed scope maps to the empty string, as the spec's examples do.
        let text = described.as_ref().map_or_else(String::new, LitStr::value);
        quote!((::std::string::String::from(#name), ::std::string::String::from(#text)))
    });

    let mut built = quote! {
        ::kynos::openapi::OAuthFlow::new([#(#scopes),*])
    };

    if let Some(url) = &flow.authorization_url {
        built = quote!(#built.with_authorization_url(#url));
    }
    if let Some(url) = &flow.token_url {
        built = quote!(#built.with_token_url(#url));
    }
    if let Some(url) = &flow.refresh_url {
        built = quote!(#built.with_refresh_url(#url));
    }
    if cfg!(feature = "openapi32") {
        if let Some(url) = &flow.device_authorization_url {
            built = quote!(#built.with_device_authorization_url(#url));
        }
    }

    built
}

/// The challenge a kind sends without being told.
///
/// `bearer`, `oauth2` and `openid_connect` answer `Bearer` (RFC 6750 section
/// 3); `basic` answers with `charset="UTF-8"` (RFC 7617 section 2). No `realm`;
/// `challenge = "..."` sets one. The other kinds have no default.
fn default_challenge(kind: &Ident) -> proc_macro2::TokenStream {
    match kind.to_string().as_str() {
        "bearer" | "oauth2" | "openid_connect" => quote!(::core::option::Option::Some("Bearer")),
        "basic" => quote!(::core::option::Option::Some(r#"Basic charset="UTF-8""#)),
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
                // A 3.2-only key is refused under 3.1 rather than dropped.
                "deprecated" if !cfg!(feature = "openapi32") => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`deprecated` writes a Security Scheme Object field that OpenAPI 3.2 \
                         introduced, and this build describes 3.1; enable the `openapi32` \
                         feature, or drop it",
                    ));
                }
                "deprecated" => args.deprecated = true,
                "carrier" => {
                    let value: Ident = meta.value()?.parse()?;
                    if value != "manual" {
                        return Err(syn::Error::new(
                            value.span(),
                            "`carrier` takes only `manual`, which leaves the `Carries` \
                             implementation to you; every other carrier follows from the kind",
                        ));
                    }
                    args.manual_carrier = true;
                }
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
/// checkable; `api_key` is checked in full here, where the spans are.
fn check_kind(
    kind: &Ident,
    meta: &syn::meta::ParseNestedMeta<'_>,
    nested: &mut Nested,
) -> syn::Result<()> {
    if meta.input.peek(syn::token::Paren) {
        let is_oauth2 = kind == "oauth2";
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
                // A flow only inside `oauth2`; elsewhere an unknown option.
                flow if is_oauth2 => read_flow(key, flow, &option, nested)?,
                _ => skip_value(&option)?,
            }
            Ok(())
        })?;
    } else {
        // A bare kind such as `bearer` has no list to read.
        skip_value(meta)?;
    }

    if kind == "oauth2" {
        return check_oauth2(kind, nested);
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
    // Header and cookie names must be tokens; a query name may be any string.
    let grammar = match location.value().as_str() {
        "header" => Some("an RFC 9110 field name"),
        "cookie" => Some("an RFC 6265 cookie name"),
        _ => None,
    };
    if let Some(message) =
        grammar.and_then(|grammar| non_token_message(&field.value(), &location.value(), grammar))
    {
        return Err(syn::Error::new(field.span(), message));
    }
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

/// Reads one declared OAuth 2.0 flow, checking `name` against [`FLOWS`] here
/// where its span is.
fn read_flow(
    key: &Ident,
    name: &str,
    option: &syn::meta::ParseNestedMeta<'_>,
    nested: &mut Nested,
) -> syn::Result<()> {
    let Some(flow) = flow_named(name) else {
        return Err(syn::Error::new(
            key.span(),
            format!(
                "`{name}` is not an OAuth 2.0 flow; the flows are {}",
                FLOWS
                    .iter()
                    .map(|flow| format!("`{}`", flow.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    };

    if flow.since_three_two && !cfg!(feature = "openapi32") {
        return Err(syn::Error::new(
            key.span(),
            format!(
                "the `{name}` flow was introduced in OpenAPI 3.2, and this build describes 3.1;                  enable the `openapi32` feature, or declare a flow 3.1 can express"
            ),
        ));
    }

    if let Some((existing, _)) = nested.flows.iter().find(|(declared, _)| declared == key) {
        return Err(syn::Error::new(
            key.span(),
            format!("the `{existing}` flow is already declared, and a scheme declares each once"),
        ));
    }

    let mut args = FlowArgs::default();
    if option.input.peek(syn::token::Paren) {
        option.parse_nested_meta(|field| {
            let Some(name) = field.path.get_ident() else {
                return skip_value(&field);
            };
            match name.to_string().as_str() {
                "authorization_url" => args.authorization_url = Some(field.value()?.parse()?),
                "token_url" => args.token_url = Some(field.value()?.parse()?),
                "device_authorization_url" => {
                    args.device_authorization_url = Some(field.value()?.parse()?);
                }
                "refresh_url" => args.refresh_url = Some(field.value()?.parse()?),
                "scopes" => {
                    let content;
                    syn::parenthesized!(content in field.input);
                    // `"a"` or `"a" = "Read a"`.
                    let scopes = content.parse_terminated(parse_scope, Token![,])?;
                    args.scopes.extend(scopes);
                }
                _ => skip_value(&field)?,
            }
            Ok(())
        })?;
    }

    nested.flows.push((key.clone(), args));
    Ok(())
}

/// One scope, with or without the description a document prints beside it.
fn parse_scope(input: syn::parse::ParseStream<'_>) -> syn::Result<(LitStr, Option<LitStr>)> {
    let name: LitStr = input.parse()?;
    let described = if input.peek(Token![=]) {
        input.parse::<Token![=]>()?;
        Some(input.parse()?)
    } else {
        None
    };
    Ok((name, described))
}

/// Checks what an OAuth 2.0 scheme declared once every flow has been read,
/// including each flow's required URLs.
fn check_oauth2(kind: &Ident, nested: &Nested) -> syn::Result<()> {
    if nested.flows.is_empty() {
        return Err(syn::Error::new(
            kind.span(),
            "an OAuth 2.0 scheme must declare at least one flow: a scheme with none describes an \
             authorization server no client can reach. Add `authorization_code(...)`, \
             `client_credentials(...)`, `password(...)` or `implicit(...)`",
        ));
    }

    if nested.metadata_url.is_some() && !cfg!(feature = "openapi32") {
        return Err(syn::Error::new(
            kind.span(),
            "`metadata_url` writes `oauth2MetadataUrl`, which OpenAPI 3.2 introduced, and this              build describes 3.1; enable the `openapi32` feature, or drop it",
        ));
    }

    for (name, args) in &nested.flows {
        let flow = flow_named(&name.to_string())
            .expect("`read_flow` refuses a flow this table does not name");
        for required in flow.required {
            if args.url(required).is_none() {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "the `{}` flow needs `{required}`: RFC 6749 defines the grant in terms of \
                         it, so a description omitting it is one no client can follow",
                        flow.name
                    ),
                ));
            }
        }
    }

    Ok(())
}
