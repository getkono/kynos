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

/// A struct's schema, which its fields decide.
///
/// A newtype is transparent, because serde makes it so: `Sku(String)` is a
/// string on the wire and describing it as anything else would be a claim the
/// serializer contradicts. A longer tuple is the array serde writes, and a unit
/// struct is `null`.
fn struct_body(fields: &Fields, container: &Container) -> TokenStream2 {
    match fields {
        Fields::Named(named) => object_body(&named.named, container, None),
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            let ty = &unnamed.unnamed[0].ty;
            quote!(registry.resolve::<#ty>())
        }
        Fields::Unnamed(unnamed) => tuple_body(&unnamed.unnamed),
        Fields::Unit => quote! {
            ::kynos::openapi::Schema::of_type(
                ::kynos::openapi::model::schema::types::SchemaType::Null,
            )
        },
    }
}

/// A tuple's schema: a closed array, one `prefixItems` entry per member.
fn tuple_body(fields: &Punctuated<Field, Comma>) -> TokenStream2 {
    let members = fields.iter().map(|field| {
        let ty = &field.ty;
        quote!(registry.resolve::<#ty>())
    });

    quote! {
        {
            let mut keywords = ::kynos::openapi::SchemaObject::default();
            keywords.ty = ::core::option::Option::Some(
                ::kynos::openapi::model::schema::types::TypeSet::One(
                    ::kynos::openapi::model::schema::types::SchemaType::Array,
                ),
            );
            keywords.prefix_items =
                ::core::option::Option::Some(::std::vec![#(#members),*]);
            // Closed, because a tuple has exactly as many members as it has.
            keywords.items = ::core::option::Option::Some(
                ::std::boxed::Box::new(::kynos::openapi::Schema::never()),
            );
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
        }
    }
}

/// An object schema over named fields, optionally carrying a tag property.
///
/// `tag` is `(property, value)` for an internally tagged enum variant, which is
/// an object whose fields are the variant's plus the one that says which
/// variant it is.
fn object_body(
    fields: &Punctuated<Field, Comma>,
    container: &Container,
    tag: Option<(&str, &str)>,
) -> TokenStream2 {
    let tagged = tag.map(|(property, value)| {
        let constant = constant_string(value);
        quote! {
            keywords.properties.insert(::std::string::String::from(#property), #constant);
            required.push(::std::string::String::from(#property));
        }
    });

    let entries = fields
        .iter()
        .filter(|field| is_described(field))
        .map(|field| {
            let ty = &field.ty;
            let wire = field_name(field, container);

            if is_flattened(field) {
                // A flattened field's properties belong to this object, and which
                // ones they are is only known once its own schema is built. `allOf`
                // is the composition that says so without naming them.
                return quote! {
                    keywords
                        .all_of
                        .get_or_insert_with(::std::vec::Vec::new)
                        .push(registry.resolve::<#ty>());
                };
            }

            let constrained = constraints(field)
                .map(|constraints| quote!(let schema = #constraints.apply(schema);));
            // A property's prose sits beside its schema, which for a named
            // field type is a `$ref` -- legal from 3.1 onward, where a schema
            // `$ref` applies its siblings. A boolean schema has nowhere to put
            // it and keeps none.
            let described = doc_string(&field.attrs).map(|doc| {
                quote! {
                    let mut schema = schema;
                    if let ::kynos::openapi::Schema::Object(property) = &mut schema {
                        property.description =
                            ::core::option::Option::Some(::std::string::String::from(#doc));
                    }
                }
            });
            let deprecated = is_deprecated(&field.attrs).then(|| {
                quote! {
                    let mut schema = schema;
                    if let ::kynos::openapi::Schema::Object(property) = &mut schema {
                        property.deprecated = ::core::option::Option::Some(true);
                    }
                }
            });
            let require = is_required(field)
                .then(|| quote!(required.push(::std::string::String::from(#wire));));

            quote! {
                {
                    let schema = registry.resolve::<#ty>();
                    #constrained
                    #described
                    #deprecated
                    keywords.properties.insert(::std::string::String::from(#wire), schema);
                }
                #require
            }
        });

    quote! {
        {
            let mut keywords = ::kynos::openapi::SchemaObject::default();
            keywords.ty = ::core::option::Option::Some(
                ::kynos::openapi::model::schema::types::TypeSet::One(
                    ::kynos::openapi::model::schema::types::SchemaType::Object,
                ),
            );
            let mut required: ::std::vec::Vec<::std::string::String> =
                ::std::vec::Vec::new();
            #tagged
            #(#entries)*
            if !required.is_empty() {
                keywords.required = ::core::option::Option::Some(required);
            }
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
        }
    }
}

/// A string schema fixed to one value, which is what a tag property is.
fn constant_string(value: &str) -> TokenStream2 {
    quote! {
        {
            let mut constant = ::kynos::openapi::SchemaObject::default();
            constant.ty = ::core::option::Option::Some(
                ::kynos::openapi::model::schema::types::TypeSet::One(
                    ::kynos::openapi::model::schema::types::SchemaType::String,
                ),
            );
            constant.const_value =
                ::core::option::Option::Some(::core::convert::Into::into(#value));
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(constant))
        }
    }
}

/// An enum's schema, which its tagging decides.
///
/// Four shapes, and which applies is read from the serde attributes rather than
/// chosen here: an enumeration of names where every variant is a unit, and
/// otherwise the `oneOf` that matches how the payload is tagged.
fn enum_body(data: &DataEnum, container: &Container) -> TokenStream2 {
    let variants: Vec<&Variant> = data
        .variants
        .iter()
        .filter(|variant| !is_skipped(&variant.attrs))
        .collect();

    // An `enum` array of names is the compact shape, and it has nowhere to put
    // a keyword about one member: JSON Schema deprecates a *schema*, and every
    // name in that array shares one. So a deprecated unit variant drops the
    // compact shape for the `oneOf` of `const` branches, which says the same
    // thing about the wire and gives each name a schema of its own to mark.
    // The alternative was emitting nothing, which is a description silently
    // disagreeing with the type it came from.
    let any_deprecated = variants.iter().any(|variant| is_deprecated(&variant.attrs));

    if container.tag.is_none()
        && !any_deprecated
        && variants
            .iter()
            .all(|variant| matches!(variant.fields, Fields::Unit))
    {
        let names = variants
            .iter()
            .map(|variant| variant_name(variant, container));
        return quote! {
            {
                let mut keywords = ::kynos::openapi::SchemaObject::default();
                keywords.ty = ::core::option::Option::Some(
                    ::kynos::openapi::model::schema::types::TypeSet::One(
                        ::kynos::openapi::model::schema::types::SchemaType::String,
                    ),
                );
                keywords.enumeration = ::core::option::Option::Some(::std::vec![
                    #(::core::convert::Into::into(#names)),*
                ]);
                ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
            }
        };
    }

    let branches = variants
        .iter()
        .map(|variant| branch(variant, container))
        .collect::<Vec<_>>();

    // A discriminator makes the choice cheap to determine rather than
    // guessable, which is the whole reason an untagged enum is refused: it
    // needs a property every branch carries, and only a tagged enum has one.
    let discriminator = container.tag.as_ref().map(|tag| {
        quote! {
            keywords.discriminator = ::core::option::Option::Some(
                ::kynos::openapi::Discriminator::new(#tag),
            );
        }
    });

    quote! {
        {
            let mut keywords = ::kynos::openapi::SchemaObject::default();
            keywords.one_of = ::core::option::Option::Some(::std::vec![#(#branches),*]);
            #discriminator
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
        }
    }
}

/// One `oneOf` branch: the variant, shaped by how the enum is tagged.
fn branch(variant: &Variant, container: &Container) -> TokenStream2 {
    let name = variant_name(variant, container);
    let deprecated = is_deprecated(&variant.attrs);
    let described = |schema: TokenStream2| {
        deprecate(
            described(schema, doc_string(&variant.attrs).as_deref()),
            deprecated,
        )
    };

    match (&container.tag, &container.content) {
        // Adjacently tagged: the tag and the payload are two properties of one
        // object.
        (Some(tag), Some(content)) => {
            let tagged = constant_string(&name);
            let payload = payload(&variant.fields, container).map(|payload| {
                quote! {
                    keywords.properties.insert(::std::string::String::from(#content), #payload);
                    required.push(::std::string::String::from(#content));
                }
            });
            described(quote! {
                {
                    let mut keywords = ::kynos::openapi::SchemaObject::default();
                    keywords.ty = ::core::option::Option::Some(
                        ::kynos::openapi::model::schema::types::TypeSet::One(
                            ::kynos::openapi::model::schema::types::SchemaType::Object,
                        ),
                    );
                    let mut required: ::std::vec::Vec<::std::string::String> =
                        ::std::vec::Vec::new();
                    keywords.properties.insert(::std::string::String::from(#tag), #tagged);
                    required.push(::std::string::String::from(#tag));
                    #payload
                    keywords.required = ::core::option::Option::Some(required);
                    ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
                }
            })
        }

        // Internally tagged: the tag is one more property of the variant's own
        // object. A newtype variant has no properties of its own to add it to,
        // so the two are composed instead.
        (Some(tag), None) => match &variant.fields {
            Fields::Named(named) => {
                described(object_body(&named.named, container, Some((tag, &name))))
            }
            Fields::Unit => described(object_body(
                &Punctuated::new(),
                container,
                Some((tag, &name)),
            )),
            Fields::Unnamed(_) => {
                let marker = object_body(&Punctuated::new(), container, Some((tag, &name)));
                let payload = payload(&variant.fields, container);
                described(quote! {
                    {
                        let mut keywords = ::kynos::openapi::SchemaObject::default();
                        keywords.all_of = ::core::option::Option::Some(
                            ::std::vec![#marker, #payload],
                        );
                        ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
                    }
                })
            }
        },

        // Externally tagged: the variant's name is the single property, and a
        // unit variant is that name as a bare string.
        (None, _) => match payload(&variant.fields, container) {
            None => described(constant_string(&name)),
            Some(payload) => described(quote! {
                {
                    let mut keywords = ::kynos::openapi::SchemaObject::default();
                    keywords.ty = ::core::option::Option::Some(
                        ::kynos::openapi::model::schema::types::TypeSet::One(
                            ::kynos::openapi::model::schema::types::SchemaType::Object,
                        ),
                    );
                    keywords.properties.insert(::std::string::String::from(#name), #payload);
                    keywords.required = ::core::option::Option::Some(
                        ::std::vec![::std::string::String::from(#name)],
                    );
                    ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
                }
            }),
        },
    }
}

/// The schema of what a variant carries, or nothing for a unit variant.
fn payload(fields: &Fields, container: &Container) -> Option<TokenStream2> {
    match fields {
        Fields::Unit => None,
        Fields::Named(named) => Some(object_body(&named.named, container, None)),
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            let ty = &unnamed.unnamed[0].ty;
            Some(quote!(registry.resolve::<#ty>()))
        }
        Fields::Unnamed(unnamed) => Some(tuple_body(&unnamed.unnamed)),
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

/// The wire name of a field: serde's `rename` if it has one, the container's
/// `rename_all` applied to the identifier otherwise.
fn field_name(field: &Field, container: &Container) -> String {
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
fn variant_name(variant: &Variant, container: &Container) -> String {
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
fn serde_rename(attrs: &[syn::Attribute]) -> Option<String> {
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
fn rename(ident: &str, style: &str) -> String {
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
fn is_described(field: &Field) -> bool {
    !is_skipped(&field.attrs) && !is_phantom(&field.ty)
}

fn is_phantom(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "PhantomData")
}

/// Whether an item carries `#[serde(skip)]` or either half of it.
fn is_skipped(attrs: &[syn::Attribute]) -> bool {
    serde_flag(attrs, &["skip", "skip_serializing", "skip_deserializing"])
}

fn is_flattened(field: &Field) -> bool {
    serde_flag(&field.attrs, &["flatten"])
}

/// Whether a property must be present.
///
/// An `Option` is optional because the type says so, and a field with a serde
/// `default` or a `skip_serializing_if` is optional because the wire form says
/// so. Anything else is required, which is what makes `required` follow from
/// the declaration rather than from an annotation that could contradict it.
fn is_required(field: &Field) -> bool {
    !is_option(&field.ty) && !serde_flag(&field.attrs, &["default", "skip_serializing_if"])
}

fn is_option(ty: &Type) -> bool {
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
fn serde_flag(attrs: &[syn::Attribute], keys: &[&str]) -> bool {
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
fn constraints(field: &Field) -> Option<TokenStream2> {
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
fn as_float(literal: &Lit) -> Option<LitFloat> {
    let (digits, span) = match literal {
        Lit::Int(value) => (value.token().to_string(), value.span()),
        Lit::Float(value) => (value.token().to_string(), value.span()),
        _ => return None,
    };
    let digits = digits.trim_end_matches(|character: char| character.is_alphabetic());
    Some(LitFloat::new(&format!("{digits}f64"), span))
}
