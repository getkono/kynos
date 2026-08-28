//! `#[derive(Schema)]`.
//!
//! The field grammar is exactly the keys of `schema::constraints::Constraints`,
//! so the attribute and the type it fills are one list and neither can grow
//! without the other:
//!
//! ```text
//! #[schema( <constraint> [, <constraint>]* )]     on a field, optional
//!
//! constraint := minimum = <number> | maximum = <number>
//!             | exclusive_minimum = <number> | exclusive_maximum = <number>
//!             | multiple_of = <number>
//!             | min_length = <integer> | max_length = <integer>
//!             | pattern = "<regex>"
//!             | min_items = <integer> | max_items = <integer>
//!             | unique_items
//! ```
//!
//! `format` is deliberately absent. It states what a value *is*, which follows
//! from the type or from nothing, so naming it here is an error that points at
//! the remedy rather than a key that quietly works.

mod attributes;
mod shape;

use attributes::{
    constraints, field_name, is_described, is_flattened, is_required, is_skipped, variant_name,
};
use shape::{enum_body, struct_body};

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Data, DataEnum, DeriveInput, Field, Fields, Lit, LitFloat, LitInt, LitStr, Type, Variant,
    parse_macro_input, punctuated::Punctuated, spanned::Spanned, token::Comma,
};

use crate::derive::common::{doc_string, is_deprecated, skip_value};

/// Keys taking a number, which may be written as an integer or a float.
const NUMERIC: &[&str] = &[
    "minimum",
    "maximum",
    "exclusive_minimum",
    "exclusive_maximum",
    "multiple_of",
];

/// Keys taking a non-negative count.
const COUNTS: &[&str] = &["min_length", "max_length", "min_items", "max_items"];

pub(crate) fn expand(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    match expand_inner(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

pub(super) fn expand_inner(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if let Data::Union(data) = &input.data {
        return Err(syn::Error::new(
            data.union_token.span(),
            "`Schema` cannot describe a union: no JSON value corresponds to one",
        ));
    }
    reject_untagged(input)?;
    check_constraints(input)?;

    let name = &input.ident;
    let generics = schema_bounded_generics(input);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Only a concrete type claims a component name. A generic one would give
    // every instantiation the same name, so `Page<User>` and `Page<Order>`
    // would collide in `components`; mangling the arguments into a legal
    // component key is the eventual answer, and inlining is the honest
    // placeholder rather than a name that is wrong.
    let component = LitStr::new(&name.to_string(), name.span());
    let named = if input.generics.type_params().next().is_some() {
        quote!(::core::option::Option::None)
    } else {
        quote!(::kynos::openapi::ComponentName::sanitized(#component).ok())
    };

    let container = Container::read(input);
    let body = body(input, &container);

    Ok(quote! {
        // A deprecated type still has to describe itself, and the impl below
        // names it. Without this, `#[deprecated]` plus `#[derive(Schema)]` is a
        // warning at the type's own definition -- an error under `-D warnings`,
        // which this workspace and many others set. serde's derives carry the
        // same allow for the same reason.
        #[allow(deprecated)]
        impl #impl_generics ::kynos::schema::Schema for #name #ty_generics #where_clause {
            fn schema(
                registry: &mut ::kynos::schema::registry::Registry,
            ) -> ::kynos::openapi::Schema {
                #body
            }

            fn name() -> ::core::option::Option<::kynos::openapi::ComponentName> {
                #named
            }
        }
    })
}

/// The input's generics, with `Schema` required of each type parameter.
///
/// serde's own default shape, and for the same reason: bounding the
/// *parameters* rather than the field types is both sufficient and narrower.
/// `Vec<T>: Schema` follows from `T: Schema` through the blanket
/// implementation, while a field-type bound would demand `PhantomData<T>:
/// Schema` — a bound nothing satisfies, on a field no schema describes, failing
/// at the handler rather than here.
///
/// Emitted now because the implementation will need it, and adding a bound
/// after the freeze breaks exactly the code this milestone invites people to
/// write.
fn schema_bounded_generics(input: &DeriveInput) -> syn::Generics {
    let mut generics = input.generics.clone();
    let parameters: Vec<syn::Ident> = generics
        .type_params()
        .map(|parameter| parameter.ident.clone())
        .collect();
    if parameters.is_empty() {
        return generics;
    }

    let clause = generics.make_where_clause();
    for parameter in parameters {
        clause
            .predicates
            .push(syn::parse_quote!(#parameter: ::kynos::schema::Schema));
    }
    generics
}

/// Validates every `#[schema(...)]` in the input.
///
/// Run before any code is emitted, so that [`constraints`] can read the same
/// lists back without checking them again — a key that reached the emitter had
/// its shape settled here, and one that did not never gets there.
fn check_constraints(input: &DeriveInput) -> syn::Result<()> {
    let fields = match &input.data {
        Data::Struct(data) => vec![&data.fields],
        Data::Enum(data) => data
            .variants
            .iter()
            .map(|variant| &variant.fields)
            .collect(),
        Data::Union(_) => return Ok(()),
    };

    for group in fields {
        let named = match group {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(unnamed) => &unnamed.unnamed,
            Fields::Unit => continue,
        };
        for field in named {
            for attr in &field.attrs {
                if attr.path().is_ident("schema") {
                    attr.parse_nested_meta(|meta| check_constraint(&meta))?;
                }
            }
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

    if name == "unique_items" {
        // A flag: `unique_items = true` would let `= false` mean something the
        // absence of the key already means.
        return if meta.input.peek(syn::Token![=]) {
            Err(syn::Error::new(
                key.span(),
                "`unique_items` is a flag; write it alone, or leave it out",
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
             `max_length`, `pattern`, `min_items`, `max_items` and `unique_items`"
        ),
    ))
}

/// `#[serde(untagged)]` has no describable decoding rule.
///
/// `anyOf` with no discriminator leaves a consumer to guess which branch a
/// payload is, and serde's first-match tie-break is not expressible in JSON
/// Schema. An internally or adjacently tagged enum becomes a `discriminator`,
/// which is.
fn reject_untagged(input: &DeriveInput) -> syn::Result<()> {
    // Only an enum can be untagged. serde refuses the attribute anywhere else
    // in its own words, and a second diagnostic calling a struct an enum is
    // this derive restating a serde shape rule and misnaming the shape.
    if !matches!(input.data, syn::Data::Enum(_)) {
        return Ok(());
    }

    for attr in &input.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut found = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("untagged") {
                found = Some(meta.path.span());
            } else {
                skip_value(&meta)?;
            }
            Ok(())
        });
        if let Some(span) = found {
            return Err(syn::Error::new(
                span,
                "an untagged enum has no describable decoding rule: `anyOf` without a \
                 discriminator is ambiguous, and serde's first-match tie-break cannot be \
                 expressed. Use `#[serde(tag = \"...\")]`, which becomes a `discriminator`",
            ));
        }
    }
    Ok(())
}

/// What the type's own serde attributes said.
///
/// Read rather than restated: `rename_all`, `tag` and `content` are already on
/// the type because it has to serialize, and a parallel `#[schema(...)]`
/// spelling of them would be a second declaration to keep in step.
#[derive(Default)]
struct Container {
    rename_all: Option<String>,
    tag: Option<String>,
    content: Option<String>,
    doc: Option<String>,
}

impl Container {
    fn read(input: &DeriveInput) -> Self {
        let mut container = Self {
            doc: doc_string(&input.attrs),
            ..Self::default()
        };

        for attr in &input.attrs {
            if !attr.path().is_ident("serde") {
                continue;
            }
            // Shape errors in serde's own attribute are serde's to report:
            // this derive reads what it recognizes and stays silent about the
            // rest, so a key it has not learned is not a second diagnostic on
            // the same line.
            let _ = attr.parse_nested_meta(|meta| {
                let Some(key) = meta.path.get_ident() else {
                    return skip_value(&meta);
                };
                match key.to_string().as_str() {
                    "rename_all" => container.rename_all = string_value(&meta)?,
                    "tag" => container.tag = string_value(&meta)?,
                    "content" => container.content = string_value(&meta)?,
                    _ => skip_value(&meta)?,
                }
                Ok(())
            });
        }

        container
    }
}

/// The `= "..."` of a nested-meta item, when it has one.
fn string_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Option<String>> {
    if !meta.input.peek(syn::Token![=]) {
        return Ok(None);
    }
    Ok(Some(meta.value()?.parse::<LitStr>()?.value()))
}

/// The `schema` body for whatever shape the type has.
fn body(input: &DeriveInput, container: &Container) -> TokenStream2 {
    let described = match &input.data {
        Data::Struct(data) => described(
            struct_body(&data.fields, container),
            container.doc.as_deref(),
        ),
        Data::Enum(data) => described(enum_body(data, container), container.doc.as_deref()),
        // Refused at the top of `expand_inner`.
        Data::Union(_) => quote!(::kynos::openapi::Schema::default()),
    };

    deprecate(described, is_deprecated(&input.attrs))
}

/// Marks the schema deprecated, where the item said so and the schema can say it.
///
/// Shaped like [`described`], and for the same reason: a boolean schema has
/// nowhere to carry a keyword, so it carries none. A `$ref` does -- from 3.1
/// onward a schema `$ref` applies its siblings -- which is what lets a
/// deprecated field whose type is a named component be marked at the field
/// rather than on the component every other field shares.
///
/// Never `Some(false)`. The specification defaults the keyword to false, so
/// writing it out states nothing and puts a word in every schema in the
/// document; `Operation::set_deprecated` already takes the same line.
fn deprecate(schema: TokenStream2, deprecated: bool) -> TokenStream2 {
    if !deprecated {
        return schema;
    }
    quote! {
        {
            let mut deprecated = #schema;
            if let ::kynos::openapi::Schema::Object(keywords) = &mut deprecated {
                keywords.deprecated = ::core::option::Option::Some(true);
            }
            deprecated
        }
    }
}

/// Attaches the type's own prose to the schema it produces.
fn described(schema: TokenStream2, doc: Option<&str>) -> TokenStream2 {
    let Some(doc) = doc else {
        return schema;
    };
    quote! {
        {
            let mut described = #schema;
            if let ::kynos::openapi::Schema::Object(keywords) = &mut described {
                keywords.description =
                    ::core::option::Option::Some(::std::string::String::from(#doc));
            }
            described
        }
    }
}

/// The names this derive would describe a struct's fields under, in order.
///
/// Read by [`multipart`](super::multipart), so that the part a body carries and
/// the property the description names come from one rule rather than two that
/// agree until a `rename_all` is added.
pub(super) fn property_names(input: &DeriveInput, fields: &syn::FieldsNamed) -> Vec<String> {
    let container = Container::read(input);
    fields
        .named
        .iter()
        .map(|field| field_name(field, &container))
        .collect()
}
