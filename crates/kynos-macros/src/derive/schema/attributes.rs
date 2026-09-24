use super::{
    COUNTS, Container, Field, Fields, Lit, LitFloat, LitInt, NUMERIC, Span, Spanned, TokenStream2,
    Type, Variant, quote, skip_value, string_value,
};

/// The wire name of a field: serde's `rename` if it has one, the container's
/// `rename_all` applied to the identifier otherwise.
pub(super) fn field_name(field: &Field, container: &Container) -> String {
    if let Some(renamed) = serde_rename(&field.attrs) {
        return renamed;
    }
    let ident = field
        .ident
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    container
        .rename_all
        .as_deref()
        .map_or(ident.clone(), |style| rename(&ident, style))
}

/// The same for a variant.
pub(super) fn variant_name(variant: &Variant, container: &Container) -> String {
    if let Some(renamed) = serde_rename(&variant.attrs) {
        return renamed;
    }
    let ident = variant.ident.to_string();
    container
        .rename_all
        .as_deref()
        .map_or(ident.clone(), |style| rename(&ident, style))
}

/// The `rename = "..."` of a `#[serde(...)]` list, if one is written.
pub(super) fn serde_rename(attrs: &[syn::Attribute]) -> Option<String> {
    let mut found = None;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                found = string_value(&meta)?;
            } else {
                skip_value(&meta)?;
            }
            Ok(())
        });
    }
    found
}

/// serde's `rename_all` styles, applied to one identifier.
pub(super) fn rename(ident: &str, style: &str) -> String {
    let words = || {
        let mut words: Vec<String> = Vec::new();
        let mut current = String::new();
        for character in ident.chars() {
            if character == '_' {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                continue;
            }
            if character.is_uppercase() && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            current.extend(character.to_lowercase());
        }
        if !current.is_empty() {
            words.push(current);
        }
        words
    };

    let capitalize = |word: &str| {
        let mut characters = word.chars();
        characters.next().map_or_else(String::new, |first| {
            first.to_uppercase().collect::<String>() + characters.as_str()
        })
    };

    match style {
        "lowercase" => ident.to_lowercase(),
        "UPPERCASE" => ident.to_uppercase(),
        "snake_case" => words().join("_"),
        "SCREAMING_SNAKE_CASE" => words().join("_").to_uppercase(),
        "kebab-case" => words().join("-"),
        "SCREAMING-KEBAB-CASE" => words().join("-").to_uppercase(),
        "PascalCase" => words().iter().map(|word| capitalize(word)).collect(),
        "camelCase" => {
            let words = words();
            let mut renamed = words.first().cloned().unwrap_or_default();
            for word in words.iter().skip(1) {
                renamed.push_str(&capitalize(word));
            }
            renamed
        }
        // A style this derive has not learned leaves the name alone, so that
        // serde owns the diagnostic for a style neither of them knows.
        _ => ident.to_owned(),
    }
}

/// Whether a named field is in the object serde reads.
///
/// serde reads a field unless it is `skip` or `skip_deserializing`, so a field it
/// only never writes is described, and one it only never reads is not.
///
/// A `PhantomData` is read like any other field: serde writes it as `null` and
/// refuses a document without it, so it is described, as the `null`
/// [`member_schema`](super::shape::member_schema) gives it. A flattened one is
/// the exception, since serde writes nothing of it into the object and reads
/// nothing from it.
pub(super) fn is_described(field: &Field) -> bool {
    !(serde_flag(&field.attrs, &["skip", "skip_deserializing"])
        || (is_phantom(&field.ty) && is_flattened(field)))
}

/// The fields a schema describes, in declaration order: each one
/// [`is_described`] keeps.
pub(super) fn described_members(fields: &Fields) -> Vec<&Field> {
    fields.iter().filter(|field| is_described(field)).collect()
}

/// The fields a `#[serde(transparent)]` struct may be written through, and the
/// fields it may be read through, in that order.
///
/// `serde_derive`'s `allow_transparent`, read from the attributes: a field is
/// written through unless it is `skip` or `skip_serializing`, read through
/// unless it is `skip`, `skip_deserializing` or given a field-level `default`,
/// and a `PhantomData` is neither. A container `default` is not read, because
/// serde does not read it there.
pub(super) fn transparent_members(fields: &Fields) -> (Vec<&Field>, Vec<&Field>) {
    let candidates = |excluded: &[&str]| {
        fields
            .iter()
            .filter(|field| !is_phantom(&field.ty) && !serde_flag(&field.attrs, excluded))
            .collect::<Vec<_>>()
    };
    (
        candidates(&["skip", "skip_serializing"]),
        candidates(&["skip", "skip_deserializing", "default"]),
    )
}

/// The one field a `#[serde(transparent)]` struct is described by: the field
/// both directions pick, or the single field of the one direction that picks
/// one.
///
/// serde refuses a derive whose direction has no single candidate, so where only
/// one direction picks a single field the struct compiles with that direction's
/// derive alone, and the field is all serde writes, or reads. Two different
/// single picks give no field, and `reject_transparent_without_one_field`
/// refuses that struct.
pub(super) fn transparent_member(fields: &Fields) -> Option<&Field> {
    let (written, read) = transparent_members(fields);
    match (written.as_slice(), read.as_slice()) {
        ([written], [read]) => std::ptr::eq(*written, *read).then_some(*written),
        ([only], _) | (_, [only]) => Some(*only),
        _ => None,
    }
}

/// Whether a type is a `PhantomData`.
///
/// A type a macro passed through a `$t:ty` fragment arrives inside an invisible
/// group, which `serde_derive`'s `ungroup` unwraps, and nothing else, before its
/// own test. This unwraps the same, so a marker is a marker however it was
/// written.
pub(super) fn is_phantom(ty: &Type) -> bool {
    let mut ty = ty;
    while let Type::Group(group) = ty {
        ty = &group.elem;
    }
    let Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "PhantomData")
}

/// Whether serde leaves a member out in both directions: `#[serde(skip)]`, or
/// `skip_serializing` beside `skip_deserializing`.
pub(super) fn is_skipped_both_ways(attrs: &[syn::Attribute]) -> bool {
    serde_flag(attrs, &["skip"])
        || (serde_flag(attrs, &["skip_serializing"]) && serde_flag(attrs, &["skip_deserializing"]))
}

/// Whether a variant is a unit on the wire: declared as one, or a newtype
/// variant whose member serde skips both ways, which serde writes and reads as
/// a unit variant.
pub(super) fn is_unit_like(fields: &Fields) -> bool {
    match fields {
        Fields::Unit => true,
        Fields::Unnamed(unnamed) => {
            unnamed.unnamed.len() == 1 && is_skipped_both_ways(&unnamed.unnamed[0].attrs)
        }
        Fields::Named(_) => false,
    }
}

pub(super) fn is_flattened(field: &Field) -> bool {
    serde_flag(&field.attrs, &["flatten"])
}

/// Whether a field carries `#[schema(open)]`, and where it says so.
///
/// The span is the `open` key itself, so a diagnostic about it points at the
/// word rather than at the whole field.
pub(super) fn open_span(field: &Field) -> Option<Span> {
    let mut found = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("schema") {
            continue;
        }
        // Shape errors in the list are `check_constraints`' to report, so this
        // reads the one key it wants and stays silent about the rest.
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("open") {
                found = Some(meta.path.span());
                return Ok(());
            }
            skip_value(&meta)
        });
    }
    found
}

/// The first of `keys` a `#[serde(...)]` list names, and where it is written.
///
/// Shaped like [`open_span`]: the span is the key itself, and shape errors in
/// the list are serde's to report, so this raises none.
pub(super) fn serde_key_span(attrs: &[syn::Attribute], keys: &[&str]) -> Option<(String, Span)> {
    let mut found = None;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            let named = meta.path.get_ident().map(ToString::to_string);
            if let Some(key) = named.filter(|key| found.is_none() && keys.contains(&key.as_str())) {
                found = Some((key, meta.path.span()));
            }
            skip_value(&meta)
        });
    }
    found
}

pub(super) fn is_open(field: &Field) -> bool {
    open_span(field).is_some()
}

/// Whether a property must be present.
///
/// An `Option` is optional because the type says so, and a field with a serde
/// `default` of its own, or in a struct whose container carries one, is
/// optional because the wire form says so in both directions: serde fills the
/// missing field from `Default` on read. Anything else is required, which is
/// what makes `required` follow from the declaration rather than from an
/// annotation that could contradict it.
///
/// `skip_serializing_if` is not read here: it only lets a field be absent from
/// what is written, and `reject_read_required_skip` refuses it on every
/// described, unflattened field of an object serde writes that this rule says
/// is still required on read. A flattened field never reaches this rule:
/// `#[schema(open)]` decides it.
pub(super) fn is_required(field: &Field, container: &Container) -> bool {
    !is_option(&field.ty) && !container.default && !serde_flag(&field.attrs, &["default"])
}

pub(super) fn is_option(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option")
}

/// Whether any `#[serde(...)]` list names one of `keys`, with or without a
/// value.
pub(super) fn serde_flag(attrs: &[syn::Attribute], keys: &[&str]) -> bool {
    let mut found = false;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta
                .path
                .get_ident()
                .is_some_and(|key| keys.contains(&key.to_string().as_str()))
            {
                found = true;
            }
            skip_value(&meta)
        });
    }
    found
}

/// A field's `#[schema(...)]` constraints, as a `Constraints` expression.
///
/// `Constraints` is `#[non_exhaustive]`, so the value is built from `default`
/// and assigned into: it grows without breaking an expansion that predates the
/// growth.
pub(super) fn constraints(field: &Field) -> Option<TokenStream2> {
    let mut assignments: Vec<TokenStream2> = Vec::new();

    for attr in &field.attrs {
        if !attr.path().is_ident("schema") {
            continue;
        }
        // Every shape here was checked by `check_constraints` before any code
        // was emitted, so a value that does not fit is already a diagnostic.
        let _ = attr.parse_nested_meta(|meta| {
            let Some(key) = meta.path.get_ident() else {
                return skip_value(&meta);
            };
            let name = key.to_string();

            // `open` is not a constraint on a value: it says how a flattened
            // field composes into the object carrying it, and `object_body`
            // reads it there.
            if name == "open" {
                return Ok(());
            }

            if name == "unique_items" {
                assignments.push(quote! {
                    constraints.unique_items = ::core::option::Option::Some(true);
                });
                return Ok(());
            }

            let field = syn::Ident::new(&name, key.span());
            let literal: Lit = meta.value()?.parse()?;

            if NUMERIC.contains(&name.as_str()) {
                if let Some(number) = as_float(&literal) {
                    assignments.push(quote! {
                        constraints.#field = ::core::option::Option::Some(#number);
                    });
                }
            } else if COUNTS.contains(&name.as_str()) {
                if let Lit::Int(count) = &literal {
                    let count = LitInt::new(&format!("{}u64", count.base10_digits()), count.span());
                    assignments.push(quote! {
                        constraints.#field = ::core::option::Option::Some(#count);
                    });
                }
            } else if name == "pattern" {
                if let Lit::Str(pattern) = &literal {
                    assignments.push(quote! {
                        constraints.pattern = ::core::option::Option::Some(
                            ::std::string::String::from(#pattern),
                        );
                    });
                }
            }

            Ok(())
        });
    }

    if assignments.is_empty() {
        return None;
    }

    Some(quote! {
        {
            let mut constraints = ::kynos::schema::constraints::Constraints::default();
            #(#assignments)*
            constraints
        }
    })
}

/// A numeric literal as an `f64` one, which is what JSON Schema bounds are.
///
/// The digits are carried across as written rather than reformatted, so a
/// bound spelled `1_000_000` stays legible in the expansion.
pub(super) fn as_float(literal: &Lit) -> Option<LitFloat> {
    let (digits, span) = match literal {
        Lit::Int(value) => (value.token().to_string(), value.span()),
        Lit::Float(value) => (value.token().to_string(), value.span()),
        _ => return None,
    };
    let digits = digits.trim_end_matches(|character: char| character.is_alphabetic());
    Some(LitFloat::new(&format!("{digits}f64"), span))
}
