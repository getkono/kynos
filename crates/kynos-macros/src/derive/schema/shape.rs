use super::{
    Comma, Container, DataEnum, Field, Fields, Punctuated, TokenStream2, Variant, constraints,
    deprecate, described, doc_string, field_name, is_deprecated, is_described, is_flattened,
    is_required, is_skipped, quote, variant_name,
};

/// A struct's schema, which its fields decide.
///
/// A newtype is transparent, because serde makes it so: `Sku(String)` is a
/// string on the wire and describing it as anything else would be a claim the
/// serializer contradicts. A longer tuple is the array serde writes, and a unit
/// struct is `null`.
pub(super) fn struct_body(fields: &Fields, container: &Container) -> TokenStream2 {
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
pub(super) fn tuple_body(fields: &Punctuated<Field, Comma>) -> TokenStream2 {
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
pub(super) fn object_body(
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
pub(super) fn constant_string(value: &str) -> TokenStream2 {
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
pub(super) fn enum_body(data: &DataEnum, container: &Container) -> TokenStream2 {
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
pub(super) fn branch(variant: &Variant, container: &Container) -> TokenStream2 {
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
pub(super) fn payload(fields: &Fields, container: &Container) -> Option<TokenStream2> {
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
