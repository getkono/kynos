use super::{
    COUNTS, Container, Field, Fields, IdentExt, Lit, LitFloat, LitInt, LitStr, NUMERIC, Span,
    Spanned, TokenStream2, Type, Variant, quote, skip_value, string_value,
};

/// The name serde writes a named field under: the serialize side of its
/// `rename`, its [`default_field_name`] otherwise.
pub(super) fn field_name(field: &Field, container: &Container) -> String {
    serde_renames(&field.attrs)
        .serialize
        .unwrap_or_else(|| default_field_name(field, container))
}

/// The name serde reads a named field under before any `alias`: the
/// deserialize side of its `rename`, its [`default_field_name`] otherwise.
pub(super) fn field_read_name(field: &Field, container: &Container) -> String {
    serde_renames(&field.attrs)
        .deserialize
        .unwrap_or_else(|| default_field_name(field, container))
}

/// The name serde writes a variant under, as [`field_name`] is for a field.
pub(super) fn variant_name(variant: &Variant, container: &Container) -> String {
    serde_renames(&variant.attrs)
        .serialize
        .unwrap_or_else(|| default_variant_name(variant, container))
}

/// The name serde reads a variant under before any `alias`, as
/// [`field_read_name`] is for a field.
pub(super) fn variant_read_name(variant: &Variant, container: &Container) -> String {
    serde_renames(&variant.attrs)
        .deserialize
        .unwrap_or_else(|| default_variant_name(variant, container))
}

/// A named field's name where no `rename` gives one: its identifier without a
/// raw identifier's `r#`, under the container's `rename_all`, which for a
/// variant's fields is [`Container::fields_of`]'s rule.
fn default_field_name(field: &Field, container: &Container) -> String {
    let ident = field
        .ident
        .as_ref()
        .map(|ident| ident.unraw().to_string())
        .unwrap_or_default();
    container
        .rename_all
        .as_deref()
        .map_or(ident.clone(), |style| rename_field(&ident, style))
}

/// The same for a variant.
fn default_variant_name(variant: &Variant, container: &Container) -> String {
    let ident = variant.ident.unraw().to_string();
    container
        .rename_all
        .as_deref()
        .map_or(ident.clone(), |style| rename_variant(&ident, style))
}

/// What one serde key gives each direction, where it gives one.
#[derive(Default)]
pub(super) struct Sides {
    pub(super) serialize: Option<String>,
    pub(super) deserialize: Option<String>,
}

/// The sides of one serde key: `key = "..."` gives both directions, and
/// `key(serialize = "...", deserialize = "...")` each side it writes.
///
/// The parenthesised form is consumed whole, so a key after it is still read.
pub(super) fn sides(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Sides> {
    let mut sides = Sides::default();
    if !meta.input.peek(syn::token::Paren) {
        let both = string_value(meta)?;
        sides.serialize.clone_from(&both);
        sides.deserialize = both;
        return Ok(sides);
    }
    meta.parse_nested_meta(|side| {
        if side.path.is_ident("serialize") {
            sides.serialize = string_value(&side)?;
        } else if side.path.is_ident("deserialize") {
            sides.deserialize = string_value(&side)?;
        } else {
            skip_value(&side)?;
        }
        Ok(())
    })?;
    Ok(sides)
}

/// The [`sides`] of the `rename` in a member's `#[serde(...)]` lists.
fn serde_renames(attrs: &[syn::Attribute]) -> Sides {
    serde_sides(attrs, "rename")
}

/// A variant's own `rename_all` style on each side, which names its fields
/// ahead of the enum's `rename_all_fields`.
pub(super) fn variant_rename_all(variant: &Variant) -> Sides {
    serde_sides(&variant.attrs, "rename_all")
}

/// The [`sides`] of `key` in `#[serde(...)]` lists, the last one written.
fn serde_sides(attrs: &[syn::Attribute], key: &str) -> Sides {
    let mut found = Sides::default();
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident(key) {
                return skip_value(&meta);
            }
            found = sides(&meta)?;
            Ok(())
        });
    }
    found
}

/// A field's identifier under a `rename_all` style: `serde_derive` 1.0.229's
/// `RenameRule::apply_to_field` (`internals/case.rs`), transcribed.
/// The identifier is read as `snake_case`; case changes are ASCII-only.
fn rename_field(field: &str, style: &str) -> String {
    let pascal = || {
        let mut pascal = String::new();
        let mut capitalize = true;
        for character in field.chars() {
            if character == '_' {
                capitalize = true;
            } else if capitalize {
                pascal.push(character.to_ascii_uppercase());
                capitalize = false;
            } else {
                pascal.push(character);
            }
        }
        pascal
    };
    match style {
        "UPPERCASE" | "SCREAMING_SNAKE_CASE" => field.to_ascii_uppercase(),
        "PascalCase" => pascal(),
        "camelCase" => lower_first(&pascal()),
        "kebab-case" => field.replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => field.to_ascii_uppercase().replace('_', "-"),
        // `lowercase` and `snake_case` are the identity for a field; an
        // unknown style is serde's to refuse.
        _ => field.to_owned(),
    }
}

/// A variant's identifier under a `rename_all` style: `serde_derive` 1.0.229's
/// `RenameRule::apply_to_variant` (`internals/case.rs`), transcribed.
/// The identifier is read as `PascalCase`, split before each uppercase letter.
fn rename_variant(variant: &str, style: &str) -> String {
    let snake = || {
        let mut snake = String::new();
        for (index, character) in variant.char_indices() {
            if index > 0 && character.is_uppercase() {
                snake.push('_');
            }
            snake.push(character.to_ascii_lowercase());
        }
        snake
    };
    match style {
        "lowercase" => variant.to_ascii_lowercase(),
        "UPPERCASE" => variant.to_ascii_uppercase(),
        "camelCase" => lower_first(variant),
        "snake_case" => snake(),
        "SCREAMING_SNAKE_CASE" => snake().to_ascii_uppercase(),
        "kebab-case" => snake().replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => snake().to_ascii_uppercase().replace('_', "-"),
        // `PascalCase` is the identity for a variant; an unknown style is
        // serde's to refuse, as for a field.
        _ => variant.to_owned(),
    }
}

/// `name` with its first character lowered in ASCII (serde panics on a
/// non-ASCII one, so that case is unobservable).
fn lower_first(name: &str) -> String {
    let mut characters = name.chars();
    characters.next().map_or_else(String::new, |first| {
        let mut lowered = String::from(first.to_ascii_lowercase());
        lowered.push_str(characters.as_str());
        lowered
    })
}

/// Whether a named field is in the object serde reads: not `skip` or
/// `skip_deserializing`, nor a flattened `PhantomData` (an unflattened one is
/// the `null` serde requires).
pub(super) fn is_described(field: &Field) -> bool {
    !(serde_flag(&field.attrs, &["skip", "skip_deserializing"])
        || (is_phantom(&field.ty) && is_flattened(field)))
}

/// The fields a schema describes, in declaration order: each one
/// [`is_described`] keeps.
pub(super) fn described_members(fields: &Fields) -> Vec<&Field> {
    fields.iter().filter(|field| is_described(field)).collect()
}

/// The field a `#[serde(transparent)]` struct is written through and the field
/// it is read through: each direction's single candidate, or `None` for none or
/// several, where serde refuses that direction's derive. Mirrors
/// `serde_derive`'s `allow_transparent`.
pub(super) fn transparent_picks(fields: &Fields) -> (Option<&Field>, Option<&Field>) {
    let pick = |excluded: &[&str]| {
        let mut candidates = fields
            .iter()
            .filter(|field| !is_phantom(&field.ty) && !serde_flag(&field.attrs, excluded));
        match (candidates.next(), candidates.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        }
    };
    (
        pick(&["skip", "skip_serializing"]),
        pick(&["skip", "skip_deserializing", "default"]),
    )
}

/// The one field a `#[serde(transparent)]` struct is described by: the field
/// both directions pick, or the field of the one direction that picks one.
pub(super) fn transparent_member(fields: &Fields) -> Option<&Field> {
    match transparent_picks(fields) {
        (Some(written), Some(read)) => std::ptr::eq(written, read).then_some(written),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// Whether a type is a `PhantomData`, unwrapping invisible groups as
/// `serde_derive`'s `ungroup` does.
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

/// Whether a field carries `#[schema(open)]`, at the `open` key.
pub(super) fn open_span(field: &Field) -> Option<Span> {
    let mut found = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("schema") {
            continue;
        }
        // Shape errors in the list are `check_constraints`' to report.
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

/// The first of `keys` a `#[serde(...)]` list names, at the key.
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

/// Whether a property must be present: not an `Option` and no field or
/// container `#[serde(default)]`. `skip_serializing_if` is not read here;
/// `reject_read_required_skip` refuses it on a required field.
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

/// Where serde takes the value it fills a missing member with.
pub(super) enum DefaultFrom {
    /// A bare `default`: the type's `Default`.
    Default,
    /// `default = "path"`: what the function at `path` returns.
    Path(syn::ExprPath),
}

/// The `default` in a `#[serde(...)]` list, and where it takes its value.
pub(super) fn serde_default(attrs: &[syn::Attribute]) -> Option<DefaultFrom> {
    let mut found = None;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("default") {
                return skip_value(&meta);
            }
            found = Some(match string_value(&meta)? {
                Some(path) => DefaultFrom::Path(syn::parse_str(&path)?),
                None => DefaultFrom::Default,
            });
            Ok(())
        });
    }
    found
}

/// One constraint a field's `#[schema(...)]` declares.
pub(super) struct Bound {
    /// The `Constraints` field it fills, which is the attribute's own key.
    pub(super) key: syn::Ident,
    /// Its value, and nothing for the `unique_items` flag.
    pub(super) value: Option<BoundValue>,
}

/// What a [`Bound`] holds, as the typed literal the expansion writes.
pub(super) enum BoundValue {
    /// An `f64`, which every numeric JSON Schema bound is.
    Number(LitFloat),
    /// A `u64` count.
    Count(LitInt),
    /// The `pattern`, as written.
    Pattern(LitStr),
}

impl quote::ToTokens for BoundValue {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match self {
            Self::Number(number) => number.to_tokens(tokens),
            Self::Count(count) => count.to_tokens(tokens),
            Self::Pattern(pattern) => pattern.to_tokens(tokens),
        }
    }
}

/// A field's `#[schema(...)]` constraints, in the order written; shared by
/// [`constraints`] and `check::value_checks` so both read the attribute alike.
pub(super) fn bounds(field: &Field) -> Vec<Bound> {
    let mut bounds = Vec::new();

    for attr in &field.attrs {
        if !attr.path().is_ident("schema") {
            continue;
        }
        // `check_constraints` already reported any malformed shape.
        let _ = attr.parse_nested_meta(|meta| {
            let Some(key) = meta.path.get_ident() else {
                return skip_value(&meta);
            };
            let name = key.to_string();

            // `open` is not a constraint; `object_body` reads it.
            if name == "open" {
                return Ok(());
            }

            if name == "unique_items" {
                bounds.push(Bound {
                    key: key.clone(),
                    value: None,
                });
                return Ok(());
            }

            let literal: Lit = meta.value()?.parse()?;
            let value = if NUMERIC.contains(&name.as_str()) {
                as_float(&literal).map(BoundValue::Number)
            } else if COUNTS.contains(&name.as_str()) {
                match &literal {
                    Lit::Int(count) => Some(BoundValue::Count(LitInt::new(
                        &format!("{}u64", count.base10_digits()),
                        count.span(),
                    ))),
                    _ => None,
                }
            } else if name == "pattern" {
                match literal {
                    Lit::Str(pattern) => Some(BoundValue::Pattern(pattern)),
                    _ => None,
                }
            } else {
                None
            };

            if let Some(value) = value {
                bounds.push(Bound {
                    key: key.clone(),
                    value: Some(value),
                });
            }
            Ok(())
        });
    }

    bounds
}

/// A field's `#[schema(...)]` constraints, as a `Constraints` expression.
/// Built from `default` and assigned into, as `Constraints` is `#[non_exhaustive]`.
pub(crate) fn constraints(field: &Field) -> Option<TokenStream2> {
    let assignments: Vec<TokenStream2> = bounds(field)
        .into_iter()
        .map(|Bound { key, value }| match value {
            None => quote! {
                constraints.#key = ::core::option::Option::Some(true);
            },
            Some(BoundValue::Pattern(pattern)) => quote! {
                constraints.#key = ::core::option::Option::Some(
                    ::std::string::String::from(#pattern),
                );
            },
            Some(value) => quote! {
                constraints.#key = ::core::option::Option::Some(#value);
            },
        })
        .collect();

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

/// A numeric literal as an `f64` one, its digits kept as written.
pub(super) fn as_float(literal: &Lit) -> Option<LitFloat> {
    let (digits, span) = match literal {
        Lit::Int(value) => (value.token().to_string(), value.span()),
        Lit::Float(value) => (value.token().to_string(), value.span()),
        _ => return None,
    };
    let digits = digits.trim_end_matches(|character: char| character.is_alphabetic());
    Some(LitFloat::new(&format!("{digits}f64"), span))
}
