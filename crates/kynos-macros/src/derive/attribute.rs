//! Reading the helper attributes' closed key sets.
//!
//! A grammar refuses a key outside it and a key written twice rather than
//! skipping the one or keeping the last of the other: either way the item
//! would silently say less than what was written.

use std::ops::RangeInclusive;

use proc_macro2::Span;
use quote::{ToTokens, quote};
use syn::{
    DeriveInput, Expr, ExprLit, Field, Lit, LitBool, LitInt, LitStr, meta::ParseNestedMeta,
    spanned::Spanned,
};

/// Tokens as written with every space dropped, normalized enough to compare
/// two spellings or name one in a diagnostic.
pub(crate) fn compact(tokens: &impl ToTokens) -> String {
    quote!(#tokens).to_string().replace(' ', "")
}

/// A nested-meta key as written, `::` and all, for matching against a closed
/// grammar and naming in its diagnostic; a qualified path matches no key.
pub(crate) fn key_name(path: &syn::Path) -> String {
    compact(path)
}

/// The keys one closed grammar has read so far, across every list it spans,
/// so a second list cannot overwrite the first.
pub(crate) struct Keys {
    seen: Vec<String>,
    /// What follows the key in the refusal of a repeat, e.g. "is already
    /// set, and `#[tag(...)]` takes each key once".
    repeat: String,
    /// The keys whose repeat is refused by a diagnostic of their own, which
    /// names what the repeat duplicates.
    own: fn(&str) -> bool,
}

impl Keys {
    /// A grammar whose every key is refused on repeat with `repeat`.
    pub(crate) fn new(repeat: impl Into<String>) -> Self {
        Self {
            seen: Vec::new(),
            repeat: repeat.into(),
            own: |_| false,
        }
    }

    /// Leaves a repeat of the keys `own` holds to their own refusals.
    pub(crate) fn except(self, own: fn(&str) -> bool) -> Self {
        Self { own, ..self }
    }

    /// The key `path` names, refused where this grammar already read it.
    pub(crate) fn read(&mut self, path: &syn::Path) -> syn::Result<String> {
        let key = key_name(path);
        if !(self.own)(&key) && self.seen.contains(&key) {
            return Err(syn::Error::new(
                path.span(),
                format!("`{key}` {}", self.repeat),
            ));
        }
        self.seen.push(key.clone());
        Ok(key)
    }
}

/// Refuses `key`, which `path` names and `grammar` does not hold; `grammar`
/// is what the refusal says it is not part of, e.g. "the `#[tag(...)]`
/// grammar".
pub(crate) fn unknown(path: &syn::Path, key: &str, grammar: &str) -> syn::Error {
    syn::Error::new(path.span(), format!("`{key}` is not part of {grammar}"))
}

/// Reads the `status = <code>` `meta` holds into `declared`, refusing a code
/// outside `range` and a second status.
///
/// `sentence` says why the range is what it is, e.g. "a handler returns the
/// final response".
pub(crate) fn status(
    meta: &ParseNestedMeta<'_>,
    range: RangeInclusive<u16>,
    sentence: &str,
    declared: &mut Option<(u16, Span)>,
) -> syn::Result<()> {
    let literal: LitInt = meta.value()?.parse()?;
    let code: u16 = literal.base10_parse()?;
    if !range.contains(&code) {
        return Err(syn::Error::new(
            literal.span(),
            format!(
                "{sentence}, so its status is between {} and {}; `{code}` is not",
                range.start(),
                range.end()
            ),
        ));
    }
    if declared.is_some() {
        return Err(syn::Error::new(
            literal.span(),
            "this already declares a status, and a response has one",
        ));
    }
    *declared = Some((code, literal.span()));
    Ok(())
}

/// What a parameter field's own attribute said.
#[derive(Default)]
pub(crate) struct FieldArgs {
    /// `rename = "..."`, which every location takes.
    pub(crate) rename: Option<LitStr>,
    /// `value = "..."`, which `#[header]` alone takes.
    pub(crate) value: Option<LitStr>,
    /// `style = "..."`, which `#[cookie]` alone takes.
    pub(crate) style: Option<LitStr>,
}

/// Reads a field's `#[param]`, `#[header]` or `#[cookie]` lists, named by
/// `attribute`, each key once across all of them.
pub(crate) fn field_args(field: &Field, attribute: &str) -> syn::Result<FieldArgs> {
    let mut args = FieldArgs::default();
    let mut keys = Keys::new(format!(
        "is already set, and `#[{attribute}(...)]` takes each key once"
    ));
    for attr in &field.attrs {
        if !attr.path().is_ident(attribute) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = keys.read(&meta.path)?;
            match key.as_str() {
                "rename" => args.rename = Some(meta.value()?.parse()?),
                "value" if attribute == "header" => args.value = Some(meta.value()?.parse()?),
                "style" if attribute == "cookie" => args.style = Some(meta.value()?.parse()?),
                _ => {
                    return Err(unknown(
                        &meta.path,
                        &key,
                        &format!("the `#[{attribute}(...)]` grammar"),
                    ));
                }
            }
            Ok(())
        })?;
    }
    Ok(args)
}

/// What a header group's own `#[header(...)]`, on the struct, said.
#[derive(Default)]
pub(crate) struct GroupArgs {
    /// `described = true | false`, the group's `HeaderParams::DESCRIBED`.
    pub(crate) described: Option<LitBool>,
}

/// Reads the `#[header]` lists on a header group's struct, each key once
/// across all of them.
///
/// The keys are the group's own, so a field key written here is refused
/// rather than applied to every field.
pub(crate) fn group_args(input: &DeriveInput) -> syn::Result<GroupArgs> {
    let mut args = GroupArgs::default();
    let mut keys = Keys::new("is already set, and `#[header(...)]` takes each key once");
    for attr in &input.attrs {
        if !attr.path().is_ident("header") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = keys.read(&meta.path)?;
            if key != "described" {
                return Err(unknown(
                    &meta.path,
                    &key,
                    "the `#[header(...)]` grammar on a struct, which takes `described` alone; a \
                     field's keys go on that field",
                ));
            }
            let value: Expr = meta.value()?.parse()?;
            let Expr::Lit(ExprLit {
                lit: Lit::Bool(flag),
                ..
            }) = value
            else {
                return Err(syn::Error::new(
                    value.span(),
                    "`described` is `true` or `false`, written as a literal: whether a group \
                     is described is settled at the derive",
                ));
            };
            args.described = Some(flag);
            Ok(())
        })?;
    }
    Ok(args)
}
