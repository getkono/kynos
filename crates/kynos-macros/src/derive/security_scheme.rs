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
//!         | scopes("a", "b") | deprecated
//! ```
//!
//! A flow's `scopes` takes either spelling: `scopes("a")` names a scope with no
//! description, and `scopes("a" = "Read a")` gives it the one the document
//! prints. The scheme-level `scopes(..)` is a different thing -- what an
//! operation demands rather than what a server publishes -- and takes names
//! only.

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

/// Every OAuth 2.0 flow, and the URLs its own grant cannot work without.
///
/// A table rather than five branches, because the flows differ only in which
/// URLs they require and the builder each maps to. RFC 6749 sections 4.1 to 4.4
/// fix the first four; RFC 8628 fixes the fifth, which OpenAPI 3.2 added.
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
    /// The OAuth 2.0 flows declared, in the order they were written.
    ///
    /// A `Vec` rather than one field per flow, so declaring one twice is a
    /// diagnostic here rather than a silent overwrite.
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

            // Set through the model's own builder, which is a no-op on any
            // scheme that has no such field -- and this one has it, since the
            // expression it is chained onto is `oauth2`.
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

/// One `OAuthFlow`, built through the model's own builders.
fn build_flow(flow: &FlowArgs) -> proc_macro2::TokenStream {
    let scopes = flow.scopes.iter().map(|(name, described)| {
        // A scope with no description still needs one in the map, and the empty
        // string is what the specification's own examples use.
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
                // A flow is only a flow inside `oauth2`. Elsewhere the same
                // word is an unknown option, and skipping it keeps every other
                // kind's grammar exactly as permissive as it was.
                flow if is_oauth2 => read_flow(key, flow, &option, nested)?,
                _ => skip_value(&option)?,
            }
            Ok(())
        })?;
    } else {
        // A kind written bare -- `bearer`, `basic`, `mutual_tls` -- has no list
        // to read.
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

/// Reads one declared OAuth 2.0 flow.
///
/// `name` is checked against [`FLOWS`] here rather than in `check_oauth2`,
/// because this is where the span of the offending word is.
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
                    // Both spellings: `"a"` names a scope, `"a" = "Read a"`
                    // gives it the description the document prints.
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

/// Checks what an OAuth 2.0 scheme declared once every flow has been read.
///
/// The per-flow URL requirement is here rather than in `read_flow` because a
/// flow's own list is only complete when its parenthesised group has closed.
fn check_oauth2(kind: &Ident, nested: &Nested) -> syn::Result<()> {
    if nested.flows.is_empty() {
        return Err(syn::Error::new(
            kind.span(),
            "an OAuth 2.0 scheme must declare at least one flow: a scheme with none describes an              authorization server no client can reach. Add `authorization_code(...)`,              `client_credentials(...)`, `password(...)` or `implicit(...)`",
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
