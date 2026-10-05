//! The `#[schema(...)]` grammar: every key a field may carry, and what each
//! takes.

use proc_macro2::Span;
use syn::{DeriveInput, Fields, Lit, LitStr};

use crate::derive::schema::{
    COUNTS, NUMERIC,
    attributes::{is_flattened, open_span},
    field_groups,
};

/// Keys written alone, with no value.
const FLAGS: &[&str] = &["unique_items", "open"];

/// Validates every `#[schema(...)]` in the input.
///
/// Run before any code is emitted, so that
/// [`constraints`](crate::derive::schema::attributes::constraints) can read the
/// same lists back without checking them again — a key that reached the emitter
/// had its shape settled here, and one that did not never gets there.
pub(super) fn check_constraints(input: &DeriveInput) -> syn::Result<()> {
    for group in field_groups(input) {
        let named = match group {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(unnamed) => &unnamed.unnamed,
            Fields::Unit => continue,
        };

        // One `unevaluatedProperties` per emitted object, so one `open` field
        // per group of fields that becomes one.
        let mut opened: Option<Span> = None;

        for field in named {
            for attr in &field.attrs {
                if attr.path().is_ident("schema") {
                    attr.parse_nested_meta(|meta| check_constraint(&meta))?;
                }
            }

            let Some(span) = open_span(field) else {
                continue;
            };

            if !is_flattened(field) {
                return Err(syn::Error::new(
                    span,
                    "`#[schema(open)]` says what a flattened field contributes to the object \
                     carrying it, and only a flattened field has anything to contribute: an \
                     ordinary field is one property, whose own schema already states what it \
                     admits. Add `#[serde(flatten)]`, or drop the attribute",
                ));
            }

            if opened.is_some() {
                return Err(syn::Error::new(
                    span,
                    "`#[schema(open)]` may appear once per container object: it supplies that \
                     object's `unevaluatedProperties`, which is one keyword, so a second open \
                     field could only overwrite what the first one said. Merge the two maps, or \
                     give one of them a named field of its own",
                ));
            }

            opened = Some(span);
        }
    }
    Ok(())
}

/// One `key` or `key = value` inside a field's `#[schema(...)]`.
fn check_constraint(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    let Some(key) = meta.path.get_ident() else {
        return Ok(());
    };
    let name = key.to_string();

    if name == "format" {
        return Err(syn::Error::new(
            key.span(),
            "`format` says what a value *is*, which follows from its type rather than from the \
             field carrying it. Use a type that already claims the format -- `uuid::Uuid` behind \
             the `uuid` feature, a date or time type behind `time-chrono` or `time-jiff`, a \
             decimal behind `decimal-rust` or `decimal-big` -- or give the value a newtype with \
             its own `Schema` implementation. `pattern` is here if what you meant is a \
             constraint on this field rather than a claim about the type",
        ));
    }

    if FLAGS.contains(&name.as_str()) {
        // A flag: `unique_items = true` would let `= false` mean something the
        // absence of the key already means.
        return if meta.input.peek(syn::Token![=]) {
            Err(syn::Error::new(
                key.span(),
                format!("`{name}` is a flag; write it alone, or leave it out"),
            ))
        } else {
            Ok(())
        };
    }

    if NUMERIC.contains(&name.as_str()) {
        return match meta.value()?.parse()? {
            Lit::Int(_) | Lit::Float(_) => Ok(()),
            other => Err(syn::Error::new(
                other.span(),
                format!("`{name}` takes a number"),
            )),
        };
    }

    if COUNTS.contains(&name.as_str()) {
        let literal = meta.value()?.parse()?;
        return match &literal {
            Lit::Int(value) => value.base10_parse::<u64>().map(|_| ()),
            other => Err(syn::Error::new(
                other.span(),
                format!("`{name}` takes a non-negative whole number"),
            )),
        };
    }

    if name == "pattern" {
        return meta.value()?.parse::<LitStr>().map(|_| ());
    }

    Err(syn::Error::new(
        key.span(),
        format!(
            "`{name}` is not part of the `#[schema(...)]` grammar, which is the keys of \
             `kynos::schema::constraints::Constraints`: `minimum`, `maximum`, \
             `exclusive_minimum`, `exclusive_maximum`, `multiple_of`, `min_length`, \
             `max_length`, `pattern`, `min_items`, `max_items` and `unique_items`; plus \
             `open`, which says a flattened field's members are not named"
        ),
    ))
}
