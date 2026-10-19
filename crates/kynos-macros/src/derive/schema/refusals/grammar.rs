//! The `#[schema(...)]` grammar: every key a field may carry, and what each
//! takes. A key outside it, a qualified key, and a key written twice on one
//! field, across lists too, are refused.

use proc_macro2::Span;
use syn::{DeriveInput, Field, Fields, Lit, LitStr, Type, spanned::Spanned};

use crate::derive::attribute::key_name;
use crate::derive::common::non_token_message;
use crate::derive::schema::{
    COUNTS, Container, NUMERIC,
    attributes::{described_as_keys, is_flattened, is_phantom, open_span},
    field_groups,
    flatten::internally_tagged_payloads,
};

/// Keys written alone, with no value.
const FLAGS: &[&str] = &["unique_items", "open"];

/// Validates every `#[schema(...)]` in the input, before any code is emitted,
/// so [`constraints`](crate::derive::schema::attributes::constraints) need not.
pub(super) fn check_constraints(input: &DeriveInput) -> syn::Result<()> {
    let payloads = internally_tagged_payloads(input, &Container::read(input));
    for group in field_groups(input) {
        let named = match group {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(unnamed) => &unnamed.unnamed,
            Fields::Unit => continue,
        };

        // One `unevaluatedProperties` per object, so one `open` field per group.
        let mut opened: Option<Span> = None;

        for field in named {
            // Across every list on the field, so a second cannot overwrite the first.
            let mut seen: Vec<String> = Vec::new();
            for attr in &field.attrs {
                if attr.path().is_ident("schema") {
                    attr.parse_nested_meta(|meta| {
                        check_once(&meta.path, &mut seen)?;
                        check_constraint(&meta)
                    })?;
                }
            }
            let payload = payloads.iter().any(|payload| std::ptr::eq(*payload, field));
            check_described_as(field, payload)?;

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

/// A field's `as`: written once, and never where serde does not write the
/// field as one value of its type. `payload` marks an internally tagged
/// newtype variant's payload, whose members serde writes beside the tag.
fn check_described_as(field: &Field, payload: bool) -> syn::Result<()> {
    let keys = described_as_keys(field);
    let Some((span, _)) = keys.first() else {
        return Ok(());
    };

    if let Some((again, _)) = keys.get(1) {
        return Err(syn::Error::new(
            *again,
            "`as` may appear once per field: a field is described as one type, so a second \
             `as` could only overwrite the first. Keep the one the wire carries",
        ));
    }

    if is_flattened(field) {
        return Err(syn::Error::new(
            *span,
            "`as` describes a field as one value of another type, and a flattened field is no \
             one value: serde writes its members into the object carrying it. Describe the \
             flattened type itself, or drop `#[serde(flatten)]`",
        ));
    }

    if payload {
        return Err(syn::Error::new(
            *span,
            "`as` describes a field as one value of another type, and an internally tagged \
             newtype variant's payload is no one value: serde writes its members beside the \
             tag, as it does a flattened field's. Describe the payload type itself",
        ));
    }

    if is_phantom(&field.ty) {
        return Err(syn::Error::new(
            *span,
            "`as` describes a field as another type, and a `PhantomData` is the `null` serde \
             writes and reads for it, whatever it is described as. Drop the key",
        ));
    }

    Ok(())
}

/// Why the check cannot enforce `pattern`, if it cannot. Gated arms in one
/// function so its mutant is compiled and tested under `all_features`.
fn enforceable(pattern: &LitStr) -> Result<(), String> {
    #[cfg(feature = "pattern")]
    {
        crate::derive::schema::pattern::translate(&pattern.value()).map(drop)
    }
    #[cfg(not(feature = "pattern"))]
    {
        let _ = pattern;
        Err(
            "`pattern` is enforced by a regular expression engine, which `kynos` compiles in \
             only under its `pattern` feature. Enable it, or drop the key: a bound nothing \
             checks would be described as though something did"
                .to_owned(),
        )
    }
}

/// Refuses a key `seen` already holds among the field's keys so far, and
/// records it otherwise. A second `as` is left to `check_described_as`, which
/// names the type it duplicates.
fn check_once(path: &syn::Path, seen: &mut Vec<String>) -> syn::Result<()> {
    let name = key_name(path);
    if name != "as" && seen.contains(&name) {
        return Err(syn::Error::new(
            path.span(),
            format!("`{name}` is already set, and a field's `#[schema(...)]` takes each key once"),
        ));
    }
    seen.push(name);
    Ok(())
}

/// One `key` or `key = value` inside a field's `#[schema(...)]`.
fn check_constraint(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    // A qualified key matches no key below, so the last refusal names it.
    let name = key_name(&meta.path);
    let span = meta.path.span();

    if name == "format" {
        return Err(syn::Error::new(
            span,
            "`format` says what a value *is*, which follows from its type rather than from the \
             field carrying it. Use a type that already claims the format -- `uuid::Uuid` behind \
             the `uuid` feature, a date or time type behind `time-chrono` or `time-jiff`, a \
             decimal behind `decimal-rust` or `decimal-big` -- or give the value a newtype with \
             its own `Schema` implementation. `pattern` is here if what you meant is a \
             constraint on this field rather than a claim about the type",
        ));
    }

    if FLAGS.contains(&name.as_str()) {
        // A flag: `= false` would only restate the key's absence.
        return if meta.input.peek(syn::Token![=]) {
            Err(syn::Error::new(
                span,
                format!("`{name}` is a flag; write it alone, or leave it out"),
            ))
        } else {
            Ok(())
        };
    }

    if NUMERIC.contains(&name.as_str()) {
        let (token, suffix, digits, span) = match meta.value()?.parse()? {
            Lit::Int(value) => (
                value.token().to_string(),
                value.suffix().to_owned(),
                value.base10_digits().to_owned(),
                value.span(),
            ),
            Lit::Float(value) => (
                value.token().to_string(),
                value.suffix().to_owned(),
                value.base10_digits().to_owned(),
                value.span(),
            ),
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    format!("`{name}` takes a number"),
                ));
            }
        };
        // The bound is emitted as these digits with `f64` appended.
        let based = ["0x", "0o", "0b"]
            .iter()
            .any(|base| token.starts_with(base));
        if suffix.is_empty() && !based {
            return Ok(());
        }
        return Err(syn::Error::new(
            span,
            format!(
                "`{name}` takes a plain decimal number, as a JSON Schema bound is written: a \
                 type suffix or a `0x`, `0o` or `0b` base says nothing the bound can carry. \
                 Write it as `{digits}`"
            ),
        ));
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
        let pattern: LitStr = meta.value()?.parse()?;
        return enforceable(&pattern).map_err(|reason| syn::Error::new(pattern.span(), reason));
    }

    if name == "content_encoding" {
        let encoding: LitStr = meta.value()?.parse()?;
        if ENCODINGS.contains(&encoding.value().as_str()) {
            return Ok(());
        }
        return Err(syn::Error::new(
            encoding.span(),
            format!(
                "{:?} is not an encoding `contentEncoding` names: JSON Schema takes them from \
                 RFC 4648 and RFC 2045, which are `base16`, `base32`, `base32hex`, `base64`, \
                 `base64url` and `quoted-printable`, written in lowercase. A string carrying \
                 its content unencoded needs no key",
                encoding.value()
            ),
        ));
    }

    if name == "content_media_type" {
        let media_type: LitStr = meta.value()?.parse()?;
        if is_media_type(&media_type.value()) {
            return Ok(());
        }
        return Err(syn::Error::new(
            media_type.span(),
            format!(
                "{:?} is not a media type, which `contentMediaType` must be: a type and a \
                 subtype, each a token, joined by `/` and optionally followed by `; name=value` \
                 parameters, as in `image/png` or `text/plain; charset=utf-8` (RFC 9110 \
                 section 8.3.1)",
                media_type.value()
            ),
        ));
    }

    if name == "as" {
        let _: Type = meta.value()?.parse()?;
        return Ok(());
    }

    Err(syn::Error::new(
        span,
        format!(
            "`{name}` is not part of the `#[schema(...)]` grammar, which is the keys of \
             `kynos::schema::constraints::Constraints`: `minimum`, `maximum`, \
             `exclusive_minimum`, `exclusive_maximum`, `multiple_of`, `min_length`, \
             `max_length`, `pattern`, `min_items`, `max_items`, `unique_items`, \
             `content_encoding` and `content_media_type`; plus `open`, which says a flattened \
             field's members are not named, and `as = T`, which describes a field as the type \
             `T`"
        ),
    ))
}

/// The `contentEncoding` values JSON Schema 2020-12 (validation, section 8.3)
/// points to: RFC 4648's, and RFC 2045's sections 6.7 and 6.8.
const ENCODINGS: &[&str] = &[
    "base16",
    "base32",
    "base32hex",
    "base64",
    "base64url",
    "quoted-printable",
];

/// Whether `value` is RFC 9110's `media-type` (section 8.3.1):
/// `type "/" subtype *( OWS ";" OWS [ parameter ] )`, each `parameter` a
/// `token "=" ( token / quoted-string )`.
fn is_media_type(value: &str) -> bool {
    const OWS: [char; 2] = [' ', '\t'];
    let token = |text: &str| non_token_message(text, "", "").is_none();

    let end = value.find([';', ' ', '\t']).unwrap_or(value.len());
    let Some((kind, subtype)) = value[..end].split_once('/') else {
        return false;
    };
    if !token(kind) || !token(subtype) {
        return false;
    }

    let mut rest = &value[end..];
    while !rest.is_empty() {
        let Some(after) = rest.trim_start_matches(OWS).strip_prefix(';') else {
            return false;
        };
        rest = after.trim_start_matches(OWS);
        // An empty parameter, which the grammar's `[ parameter ]` allows.
        if rest.is_empty() || rest.starts_with(';') {
            continue;
        }
        let Some((name, after)) = rest.split_once('=') else {
            return false;
        };
        if !token(name) {
            return false;
        }
        rest = if let Some(quoted) = after.strip_prefix('"') {
            let Some(after) = after_quoted_string(quoted) else {
                return false;
            };
            after
        } else {
            let end = after.find([';', ' ', '\t']).unwrap_or(after.len());
            if !token(&after[..end]) {
                return false;
            }
            &after[end..]
        };
    }
    true
}

/// What follows a `quoted-string` (RFC 9110 section 5.6.4) whose opening quote
/// has been read, or `None` where it never closes or holds a control character.
fn after_quoted_string(text: &str) -> Option<&str> {
    let mut characters = text.char_indices();
    while let Some((index, character)) = characters.next() {
        let character = match character {
            '"' => return Some(&text[index + 1..]),
            '\\' => characters.next()?.1,
            other => other,
        };
        if character.is_ascii_control() && character != '\t' {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests;
