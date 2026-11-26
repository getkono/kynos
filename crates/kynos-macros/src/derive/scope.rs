//! `#[derive(Scope)]`.
//!
//! ```text
//! #[scope(rename_all = "<serde rename_all style>")]   on the enum, optional
//! #[scope(rename = "<scope>")]                        on a variant, optional
//! ```
//!
//! A variant's scope is the name serde writes it under, read by the rule the
//! `Schema` and `ParamValue` derives read it by, so the enum a configuration
//! parses and the scopes a description and a token carry are one set of
//! strings. `#[scope(...)]` names an enum serde does not: each key is refused
//! beside its serde counterpart, since two names for one scope are the drift
//! the derive exists to remove. A name that is not an RFC 6749 section 3.3
//! `scope-token`, and two variants naming one scope, are refused.

use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Variant, ext::IdentExt, spanned::Spanned};

use crate::derive::{
    attribute::{Keys, unknown},
    schema::{
        attributes::{rename_variant, serde_key_span},
        members::variant_wire_names,
    },
};

/// The styles serde's `rename_all` takes, which `#[scope(rename_all)]` takes
/// too.
const STYLES: &[&str] = &[
    "lowercase",
    "UPPERCASE",
    "PascalCase",
    "camelCase",
    "snake_case",
    "SCREAMING_SNAKE_CASE",
    "kebab-case",
    "SCREAMING-KEBAB-CASE",
];

pub(crate) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(data) => return Err(not_an_enum(data.struct_token.span())),
        Data::Union(data) => return Err(not_an_enum(data.union_token.span())),
    };

    if let Some(variant) = data
        .variants
        .iter()
        .find(|variant| !matches!(variant.fields, Fields::Unit))
    {
        return Err(syn::Error::new(
            variant.fields.span(),
            "a scope is one name, so a variant declaring fields has nothing for them to say; \
             make every variant a unit",
        ));
    }

    let rename_all = rename_all(input)?;
    let variants: Vec<_> = data.variants.iter().collect();
    let mut names = Vec::with_capacity(variants.len());
    for (variant, wire, _) in variant_wire_names(input, &variants)? {
        let name = name(variant, wire, rename_all.as_ref())?;
        check_name(variant, &name, &names)?;
        names.push((variant, name));
    }

    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let arms = names.iter().map(|(variant, name)| {
        let variant = &variant.ident;
        let name = LitStr::new(name, variant.span());
        quote!(Self::#variant => #name,)
    });

    Ok(quote! {
        // A `#[deprecated]` enum or variant is named by every arm.
        #[automatically_derived]
        #[allow(deprecated)]
        impl #impl_generics ::kynos::security::Scope for #ident #ty_generics #where_clause {
            fn name(&self) -> &'static str {
                match *self {
                    #(#arms)*
                }
            }
        }
    })
}

/// The refusal of a struct or a union, at its keyword.
fn not_an_enum(span: proc_macro2::Span) -> syn::Error {
    syn::Error::new(
        span,
        "`Scope` names each variant of an enum as one scope, so a misspelt scope is an unknown \
         variant; derive it on a field-less enum",
    )
}

/// The enum's `#[scope(rename_all = "...")]`, refused beside serde's.
fn rename_all(input: &DeriveInput) -> syn::Result<Option<String>> {
    let mut style = None;
    let mut keys = Keys::new("is already set, and `#[scope(...)]` takes each key once");
    for attr in &input.attrs {
        if !attr.path().is_ident("scope") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = keys.read(&meta.path)?;
            if key != "rename_all" {
                return Err(unknown(
                    &meta.path,
                    &key,
                    "the `#[scope(...)]` grammar on an enum, which takes `rename_all` alone; a \
                     variant's `rename` goes on that variant",
                ));
            }
            let value: LitStr = meta.value()?.parse()?;
            if !STYLES.contains(&value.value().as_str()) {
                return Err(syn::Error::new(
                    value.span(),
                    format!(
                        "`rename_all` takes a style serde's does: {}",
                        STYLES.join(", ")
                    ),
                ));
            }
            if serde_key_span(&input.attrs, &["rename_all"]).is_some() {
                return Err(syn::Error::new(
                    meta.path.span(),
                    "the enum's `#[serde(rename_all)]` already names its scopes, and a second \
                     style would give each scope a name serde does not read; drop one",
                ));
            }
            style = Some(value.value());
            Ok(())
        })?;
    }
    Ok(style)
}

/// A variant's scope: its `#[scope(rename)]`, else the name serde writes it
/// under, else `rename_all`'s style over its identifier.
fn name(variant: &Variant, wire: String, rename_all: Option<&String>) -> syn::Result<String> {
    let mut renamed = None;
    let mut keys = Keys::new("is already set, and `#[scope(...)]` takes each key once");
    for attr in &variant.attrs {
        if !attr.path().is_ident("scope") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = keys.read(&meta.path)?;
            if key != "rename" {
                return Err(unknown(
                    &meta.path,
                    &key,
                    "the `#[scope(...)]` grammar on a variant, which takes `rename` alone; \
                     `rename_all` goes on the enum",
                ));
            }
            let value: LitStr = meta.value()?.parse()?;
            if serde_key_span(&variant.attrs, &["rename"]).is_some() {
                return Err(syn::Error::new(
                    meta.path.span(),
                    "the variant's `#[serde(rename)]` already names its scope, and a second \
                     name would be one serde does not read; drop one",
                ));
            }
            renamed = Some(value.value());
            Ok(())
        })?;
    }
    if let Some(renamed) = renamed {
        return Ok(renamed);
    }
    match rename_all {
        Some(style) if serde_key_span(&variant.attrs, &["rename"]).is_none() => {
            Ok(rename_variant(&variant.ident.unraw().to_string(), style))
        }
        _ => Ok(wire),
    }
}

/// Refuses a name that is not an RFC 6749 section 3.3 `scope-token`, which a
/// token's space-separated `scope` list could not carry, and one an earlier
/// variant already names.
fn check_name(variant: &Variant, name: &str, earlier: &[(&Variant, String)]) -> syn::Result<()> {
    let refused = name.chars().find(|&character| !is_nqchar(character));
    if name.is_empty() || refused.is_some() {
        let what = refused.map_or_else(
            || "is empty".to_owned(),
            |character| format!("`{name}` contains {character:?}"),
        );
        return Err(syn::Error::new(
            variant.ident.span(),
            format!(
                "this scope {what}, and a scope is an RFC 6749 `scope-token`: one or more \
                 printable ASCII characters other than space, `\"` and `\\`; name it with \
                 `#[scope(rename = \"...\")]`"
            ),
        ));
    }
    if let Some((first, _)) = earlier.iter().find(|(_, seen)| seen == name) {
        return Err(syn::Error::new(
            variant.ident.span(),
            format!(
                "two variants name the scope `{name}`; the first is `{}`",
                first.ident
            ),
        ));
    }
    Ok(())
}

/// RFC 6749 appendix A's `NQCHAR`: `%x21 / %x23-5B / %x5D-7E`.
fn is_nqchar(character: char) -> bool {
    matches!(character, '\x21' | '\x23'..='\x5B' | '\x5D'..='\x7E')
}
