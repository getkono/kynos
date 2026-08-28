use super::{
    COUNTS, Container, Field, Lit, LitFloat, LitInt, NUMERIC, TokenStream2, Type, Variant, quote,
    skip_value, string_value,
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

/// Whether a field reaches the wire at all.
///
/// `PhantomData` is skipped whatever serde does with it: it carries no value a
/// consumer can act on, and requiring `PhantomData<T>: Schema` -- which nothing
/// satisfies -- would make a marker field cost a bound the type cannot meet.
pub(super) fn is_described(field: &Field) -> bool {
    !is_skipped(&field.attrs) && !is_phantom(&field.ty)
}

pub(super) fn is_phantom(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "PhantomData")
}

/// Whether an item carries `#[serde(skip)]` or either half of it.
pub(super) fn is_skipped(attrs: &[syn::Attribute]) -> bool {
    serde_flag(attrs, &["skip", "skip_serializing", "skip_deserializing"])
}

pub(super) fn is_flattened(field: &Field) -> bool {
    serde_flag(&field.attrs, &["flatten"])
}

/// Whether a property must be present.
///
/// An `Option` is optional because the type says so, and a field with a serde
/// `default` or a `skip_serializing_if` is optional because the wire form says
/// so. Anything else is required, which is what makes `required` follow from
/// the declaration rather than from an annotation that could contradict it.
pub(super) fn is_required(field: &Field) -> bool {
    !is_option(&field.ty) && !serde_flag(&field.attrs, &["default", "skip_serializing_if"])
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
