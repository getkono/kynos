//! Reading the helper attributes' closed key sets.
//!
//! A grammar refuses a key outside it and a key written twice rather than
//! skipping the one or keeping the last of the other: either way the item
//! would silently say less than what was written.

use quote::quote;
use syn::{Field, LitStr, spanned::Spanned};

/// A nested-meta key as written, `::` and all, for matching against a closed
/// grammar and naming in its diagnostic; a qualified path matches no key.
pub(crate) fn key_name(path: &syn::Path) -> String {
    quote!(#path).to_string().replace(' ', "")
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
    let mut seen: Vec<String> = Vec::new();
    for attr in &field.attrs {
        if !attr.path().is_ident(attribute) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = key_name(&meta.path);
            if seen.contains(&key) {
                return Err(syn::Error::new(
                    meta.path.span(),
                    format!(
                        "`{key}` is already set, and `#[{attribute}(...)]` takes each key once"
                    ),
                ));
            }
            match key.as_str() {
                "rename" => args.rename = Some(meta.value()?.parse()?),
                "value" if attribute == "header" => args.value = Some(meta.value()?.parse()?),
                "style" if attribute == "cookie" => args.style = Some(meta.value()?.parse()?),
                _ => {
                    return Err(syn::Error::new(
                        meta.path.span(),
                        format!("`{key}` is not part of the `#[{attribute}(...)]` grammar"),
                    ));
                }
            }
            seen.push(key);
            Ok(())
        })?;
    }
    Ok(args)
}
