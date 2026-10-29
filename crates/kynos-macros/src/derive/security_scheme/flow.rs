//! The OAuth 2.0 flows: the table each declared flow is checked against, how
//! one is read from `oauth2(...)`, and how it is built.

use quote::quote;
use syn::{Ident, LitStr, Token, spanned::Spanned};

use super::args::Nested;
use crate::derive::attribute::{Keys, unknown};

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
pub(super) struct Flow {
    /// How the flow is spelled in the attribute.
    name: &'static str,
    /// The `OAuthFlows` builder that attaches it.
    pub(super) builder: &'static str,
    /// The URL keys this grant cannot work without.
    required: &'static [&'static str],
    /// Whether only OpenAPI 3.2 can express it.
    since_three_two: bool,
}

/// The row `name` names.
pub(super) fn flow_named(name: &str) -> Option<&'static Flow> {
    FLOWS.iter().find(|flow| flow.name == name)
}

/// What one declared flow said.
#[derive(Default)]
pub(super) struct FlowArgs {
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

/// One `OAuthFlow`, built through the model's own builders.
pub(super) fn build_flow(flow: &FlowArgs) -> proc_macro2::TokenStream {
    let scopes = flow.scopes.iter().map(|(name, described)| {
        // An undescribed scope maps to the empty string, as the spec's examples do.
        let described = super::text(described.as_ref());
        quote! {(
            <::std::string::String as ::core::convert::From<_>>::from(#name),
            <::std::string::String as ::core::convert::From<_>>::from(#described),
        )}
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
    // Only the device grant carries one, and `read_flow` refuses that flow
    // under 3.1, so a 3.1 build never reaches the 3.2-only builder.
    if let Some(url) = &flow.device_authorization_url {
        built = quote!(#built.with_device_authorization_url(#url));
    }

    built
}

/// Reads one declared OAuth 2.0 flow, checking `name` against [`FLOWS`] here
/// where its span is.
pub(super) fn read_flow(
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
                "the `{name}` flow was introduced in OpenAPI 3.2, and this build describes 3.1; \
                 enable the `openapi32` feature, or declare a flow 3.1 can express"
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
        let mut keys = Keys::new(format!(
            "is already set, and `{name}(...)` takes each key once"
        ));
        option.parse_nested_meta(|field| {
            let key = keys.read(&field.path)?;
            let span = field.path.span();
            match key.as_str() {
                // The OAuth Flow Object applies the key to the device grant
                // alone, the one flow whose row requires it.
                "device_authorization_url"
                    if !flow.required.contains(&"device_authorization_url") =>
                {
                    return Err(syn::Error::new(
                        span,
                        format!(
                            "`device_authorization_url` applies only to the `device_authorization` \
                             flow, and the `{}` flow has no device authorization endpoint; drop it",
                            flow.name
                        ),
                    ));
                }
                // The other URLs apply to the flows whose row requires them,
                // as the OAuth Flow Object's "Applies To" column says.
                "authorization_url" if flow.required.contains(&"authorization_url") => {
                    args.authorization_url = Some(field.value()?.parse()?);
                }
                "token_url" if flow.required.contains(&"token_url") => {
                    args.token_url = Some(field.value()?.parse()?);
                }
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
                _ => {
                    return Err(unknown(
                        &field.path,
                        &key,
                        &format!("the `{name}(...)` grammar"),
                    ));
                }
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
pub(super) fn check_oauth2(kind: &Ident, nested: &Nested) -> syn::Result<()> {
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
            "`metadata_url` writes `oauth2MetadataUrl`, which OpenAPI 3.2 introduced, and this \
             build describes 3.1; enable the `openapi32` feature, or drop it",
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
