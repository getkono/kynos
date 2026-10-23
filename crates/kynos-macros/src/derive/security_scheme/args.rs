//! The `#[security(...)]` grammar: the options, each kind's nested options,
//! and the checks an API key and an `http` scheme need where their spans are.

use kynos_openapi::model::parameter::header::is_ignored_header_parameter;
use syn::{DeriveInput, Expr, Ident, LitStr, Token, Type, spanned::Spanned};

use super::flow::{FlowArgs, check_oauth2, read_flow};
use crate::derive::{attribute::key_name, common::non_token_message};

/// Locations an API key may travel in; not `path` or 3.2's `querystring`.
const API_KEY_LOCATIONS: &[&str] = &["header", "query", "cookie"];

/// What the attribute said, before it becomes a description.
#[derive(Default)]
pub(super) struct SchemeArgs {
    pub(super) kind: Option<Ident>,
    pub(super) name: Option<LitStr>,
    pub(super) credential: Option<Type>,
    /// `scope = Type`: the scheme's `SecurityScheme::Scope`.
    pub(super) scope: Option<Type>,
    pub(super) challenge: Option<LitStr>,
    pub(super) description: Option<LitStr>,
    pub(super) deprecated: bool,
    /// `unavailable`: sets `SecurityScheme::MAY_BE_UNAVAILABLE`, declaring the
    /// 503 an authenticator answers when its credential store is unreachable.
    pub(super) unavailable: bool,
    /// What `carrier = ...` said, if it was written.
    pub(super) carrier: Option<Carrier>,
    /// The scheme-level `scopes(..)`: values of `scope`.
    pub(super) scopes: Vec<Expr>,
    pub(super) nested: Nested,
}

/// What `carrier = ...` asks of the derived `Carries`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Carrier {
    /// None at all: the application implements it.
    Manual,
    /// The kind's reader, presenting `Option<_>` so absence is the
    /// authenticator's to decide.
    Optional,
}

/// The options written inside a kind, whichever kind it was; one flat set,
/// since each key means the same thing wherever it is legal.
#[derive(Default)]
pub(super) struct Nested {
    pub(super) location: Option<LitStr>,
    pub(super) field: Option<LitStr>,
    pub(super) scheme: Option<LitStr>,
    pub(super) format: Option<LitStr>,
    pub(super) url: Option<LitStr>,
    pub(super) metadata_url: Option<LitStr>,
    /// The OAuth 2.0 flows declared, in order; a `Vec` so a repeat is caught.
    pub(super) flows: Vec<(Ident, FlowArgs)>,
}

/// Reads every `#[security(...)]` list on the type, refusing what the
/// grammar does not hold.
pub(super) fn parse_args(input: &DeriveInput) -> syn::Result<SchemeArgs> {
    let mut args = SchemeArgs::default();
    // Across every list, so a second list cannot overwrite the first.
    let mut seen: Vec<String> = Vec::new();

    for attr in &input.attrs {
        if !attr.path().is_ident("security") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = key_name(&meta.path);
            let span = meta.path.span();
            // `carrier` and the kind name what they repeat, below.
            if seen.contains(&key) && key != "carrier" && !is_kind(&key) {
                return Err(syn::Error::new(
                    span,
                    format!("`{key}` is already set, and `#[security(...)]` takes each key once"),
                ));
            }

            match key.as_str() {
                "name" => args.name = Some(meta.value()?.parse()?),
                "credential" => args.credential = Some(meta.value()?.parse()?),
                "scope" => args.scope = Some(meta.value()?.parse()?),
                "challenge" => args.challenge = Some(meta.value()?.parse()?),
                "description" => args.description = Some(meta.value()?.parse()?),
                // A 3.2-only key is refused under 3.1 rather than dropped.
                "deprecated" if !cfg!(feature = "openapi32") => {
                    return Err(syn::Error::new(
                        span,
                        "`deprecated` writes a Security Scheme Object field that OpenAPI 3.2 \
                         introduced, and this build describes 3.1; enable the `openapi32` \
                         feature, or drop it",
                    ));
                }
                "deprecated" => args.deprecated = true,
                "unavailable" => args.unavailable = true,
                "carrier" => {
                    if args.carrier.is_some() {
                        return Err(syn::Error::new(
                            span,
                            "`carrier` is already set, and a scheme has exactly one carrier",
                        ));
                    }
                    let value: Ident = meta.value()?.parse()?;
                    args.carrier = Some(if value == "manual" {
                        Carrier::Manual
                    } else if value == "optional" {
                        Carrier::Optional
                    } else {
                        return Err(syn::Error::new(
                            value.span(),
                            "`carrier` takes `manual`, which leaves the `Carries` implementation \
                             to you, or `optional`, which hands an absent credential to the \
                             authenticator; every other carrier follows from the kind",
                        ));
                    });
                }
                "scopes" => {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    args.scopes.extend(
                        content.parse_terminated(<Expr as syn::parse::Parse>::parse, Token![,])?,
                    );
                }
                kind if is_kind(kind) => {
                    if let Some(existing) = &args.kind {
                        return Err(syn::Error::new(
                            span,
                            format!(
                                "a security scheme has exactly one kind, and this one is already \
                                 `{existing}`"
                            ),
                        ));
                    }
                    let kind = meta.path.require_ident()?;
                    check_kind(kind, &meta, &mut args.nested)?;
                    args.kind = Some(kind.clone());
                }
                other => {
                    return Err(syn::Error::new(
                        span,
                        format!("`{other}` is not part of the `#[security(...)]` grammar"),
                    ));
                }
            }
            seen.push(key);
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
/// checkable; `api_key` and `http` are checked in full here, where the spans
/// are.
fn check_kind(
    kind: &Ident,
    meta: &syn::meta::ParseNestedMeta<'_>,
    nested: &mut Nested,
) -> syn::Result<()> {
    if meta.input.peek(syn::token::Paren) {
        let kind_name = kind.to_string();
        meta.parse_nested_meta(|option| {
            let key = key_name(&option.path);
            // The options this kind models; any other is refused, not skipped.
            let slot = match (kind_name.as_str(), key.as_str()) {
                ("api_key", "in") => Some(&mut nested.location),
                ("api_key", "name") => Some(&mut nested.field),
                ("http", "scheme") => Some(&mut nested.scheme),
                ("bearer" | "http", "format") => Some(&mut nested.format),
                ("openid_connect", "url") => Some(&mut nested.url),
                ("oauth2", "metadata_url") => Some(&mut nested.metadata_url),
                _ => None,
            };
            if let Some(slot) = slot {
                if slot.is_some() {
                    return Err(syn::Error::new(
                        option.path.span(),
                        format!(
                            "`{key}` is already set, and `{kind_name}(...)` takes each option once"
                        ),
                    ));
                }
                *slot = Some(option.value()?.parse()?);
                return Ok(());
            }
            // Every other option of `oauth2` is a flow, which `read_flow` checks.
            match option.path.get_ident() {
                Some(flow) if kind_name == "oauth2" => read_flow(flow, &key, &option, nested),
                _ => Err(syn::Error::new(
                    option.path.span(),
                    format!("`{key}` is not part of the `{kind_name}(...)` grammar"),
                )),
            }
        })?;
    } else if meta.input.peek(Token![=]) {
        // A kind's options go in its list; a value here would be dropped. A
        // kind with no options is pointed at its bare form, since its list
        // refuses every option too.
        let message = if kind == "basic" || kind == "mutual_tls" {
            format!("`{kind}` takes no options; write it bare, as `{kind}`, not with a value")
        } else {
            format!("`{kind}` takes its options in a list, as `{kind}(...)`, not a value")
        };
        return Err(syn::Error::new(meta.path.span(), message));
    }

    if kind == "oauth2" {
        return check_oauth2(kind, nested);
    }

    if kind == "http" {
        return check_http(kind, nested);
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
    // The OpenAPI Parameter Object says such a definition shall be ignored.
    if location.value() == "header" && is_ignored_header_parameter(&field.value()) {
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

/// An `http` kind's `scheme` is the Security Scheme Object's REQUIRED field
/// and the 401's challenge, so it must be an RFC 9110 `auth-scheme` token.
fn check_http(kind: &Ident, nested: &Nested) -> syn::Result<()> {
    let Some(scheme) = &nested.scheme else {
        return Err(syn::Error::new(
            kind.span(),
            "an `http` scheme must say which authentication scheme it is: `scheme = \"Digest\"`",
        ));
    };
    match non_token_message(&scheme.value(), "scheme", "an RFC 9110 auth-scheme") {
        Some(message) => Err(syn::Error::new(scheme.span(), message)),
        None => Ok(()),
    }
}
