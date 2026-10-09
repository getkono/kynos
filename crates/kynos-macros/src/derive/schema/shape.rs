use super::{
    Comma, Container, DataEnum, Field, Fields, Punctuated, TokenStream2, Variant,
    aliases::{keyed, named_string, property, variants_read_names},
    closed, constraints, deprecate, described, described_variants, doc_string, is_deprecated,
    is_described, is_flattened, is_open, is_phantom, is_unit_like, min_items, positional_members,
    quote, transparent_member,
};

/// A struct's schema, which its fields decide.
///
/// A transparent struct or newtype is its member's schema, a longer tuple an
/// array, a unit struct `null`. A named struct's `#[serde(tag = "...")]` is a
/// required property whose `const` is `name`: serde writes it and ignores it
/// on read.
pub(super) fn struct_body(fields: &Fields, container: &Container, name: &str) -> TokenStream2 {
    if let (true, Some(field)) = (container.transparent, transparent_member(fields)) {
        return member_schema(field);
    }

    match fields {
        Fields::Named(named) => {
            let written = [name.to_owned()];
            let tag = container
                .tag
                .as_deref()
                .filter(|_| container.content.is_none())
                .map(|tag| (tag, written.as_slice()));
            object_body(&named.named, container, tag)
        }
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            member_schema(&unnamed.unnamed[0])
        }
        Fields::Unnamed(unnamed) => tuple_body(&unnamed.unnamed, container.default),
        Fields::Unit => quote! {
            ::kynos::openapi::Schema::of_type(
                ::kynos::openapi::model::schema::types::SchemaType::Null,
            )
        },
    }
}

/// A tuple's schema: a closed array of the members serde does not skip both
/// ways, each under what it declares, bounded below by the fewest serde reads,
/// which a container default (`defaulted`) lowers to none. With no member left
/// there is no `prefixItems`.
pub(super) fn tuple_body(fields: &Punctuated<Field, Comma>, defaulted: bool) -> TokenStream2 {
    let positions = positional_members(fields);
    let fewest = min_items(&positions, defaulted);
    let members = positions.iter().map(|field| member_schema(field));
    let prefix = (!positions.is_empty()).then(|| {
        quote!(keywords.prefix_items = ::core::option::Option::Some(::std::vec![#(#members),*]);)
    });
    let bound =
        (fewest > 0).then(|| quote!(keywords.min_items = ::core::option::Option::Some(#fewest);));

    quote! {
        {
            let mut keywords = ::kynos::openapi::SchemaObject::default();
            keywords.ty = ::core::option::Option::Some(
                ::kynos::openapi::model::schema::types::TypeSet::One(
                    ::kynos::openapi::model::schema::types::SchemaType::Array,
                ),
            );
            #prefix
            // Closed, because a tuple has exactly as many members as it has.
            keywords.items = ::core::option::Option::Some(
                ::std::boxed::Box::new(::kynos::openapi::Schema::never()),
            );
            #bound
            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
        }
    }
}

/// An object schema over named fields, optionally carrying a tag property.
///
/// `tag` is `(property, names)`: every read name for an internally tagged
/// variant, the written one for a tagged struct. Closed per [`closed`](super::closed).
pub(super) fn object_body(
    fields: &Punctuated<Field, Comma>,
    container: &Container,
    tag: Option<(&str, &[String])>,
) -> TokenStream2 {
    let tagged = tag.map(|(property, names)| {
        let constant = named_string(names);
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

            if is_flattened(field) {
                if is_open(field) {
                    // Hoisted to `unevaluatedProperties` on the parent: inside an
                    // `allOf` branch `additionalProperties` would also reach the
                    // members this object declares itself.
                    return quote! {
                        {
                            let mut flattened = registry.resolve::<#ty>();
                            if let ::kynos::openapi::Schema::Object(open) = &mut flattened {
                                keywords.unevaluated_properties =
                                    open.additional_properties.take();
                                // Dropped: in the branch `propertyNames` would
                                // reach this object's own keys (`docs/schema.md`).
                                open.property_names = ::core::option::Option::None;
                            }
                            keywords
                                .all_of
                                .get_or_insert_with(::std::vec::Vec::new)
                                .push(flattened);
                        }
                    };
                }

                // Composed by `allOf`; sound only because `flatten_witnesses`
                // asserts the type is `Flatten`.
                return quote! {
                    keywords
                        .all_of
                        .get_or_insert_with(::std::vec::Vec::new)
                        .push(registry.resolve::<#ty>());
                };
            }

            property(field, container)
        });

    let object = quote! {
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
    };
    closed(object, container)
}

/// One described field's schema: its type's, under the field's constraints,
/// prose and deprecation.
///
/// Prose beside a `$ref` is legal from 3.1, which applies its siblings. A
/// `PhantomData` is resolved as `()`, the `null` serde uses for it.
pub(super) fn member_schema(field: &Field) -> TokenStream2 {
    let ty = &field.ty;
    let constrained =
        constraints(field).map(|constraints| quote!(let schema = #constraints.apply(schema);));
    let ty = if is_phantom(ty) {
        quote!(())
    } else {
        quote!(#ty)
    };
    let resolved = quote! {
        {
            let schema = registry.resolve::<#ty>();
            #constrained
            schema
        }
    };
    deprecate(
        described(resolved, doc_string(&field.attrs).as_deref()),
        is_deprecated(&field.attrs),
    )
}

/// An enum's schema: an `enum` of names where every variant is a unit,
/// otherwise a `oneOf` shaped by the serde tagging.
pub(super) fn enum_body(data: &DataEnum, container: &Container) -> TokenStream2 {
    let variants = described_variants(data);

    // A deprecated unit variant needs its own schema to mark, so it forces the
    // `oneOf` of `const` branches over the compact `enum`.
    let any_deprecated = variants.iter().any(|variant| is_deprecated(&variant.attrs));

    if container.tag.is_none()
        && !any_deprecated
        && variants.iter().all(|variant| is_unit_like(&variant.fields))
    {
        // Every name serde reads, each once, since no two variants read one.
        let names = variants_read_names(&variants, container)
            .into_iter()
            .flatten();
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
        .zip(variants_read_names(&variants, container))
        .map(|(variant, read)| branch(variant, &read, container))
        .collect::<Vec<_>>();

    // No `mapping`: every branch is inline, so each tag value reaches its
    // branch through the tag property's own `const` or `enum`.
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

/// One `oneOf` branch: the variant, shaped by how the enum is tagged, under
/// `read`, every name serde reads as it. Fields are named under
/// [`Container::fields_of`].
pub(super) fn branch(variant: &Variant, read: &[String], container: &Container) -> TokenStream2 {
    let fields = container.fields_of(variant);
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
            let tagged = named_string(read);
            let payload = payload(&variant.fields, &fields).map(|payload| {
                quote! {
                    keywords.properties.insert(::std::string::String::from(#content), #payload);
                    required.push(::std::string::String::from(#content));
                }
            });
            let object = quote! {
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
            };
            described(closed(object, container))
        }

        // Internally tagged: the tag joins the variant's object; a newtype's
        // payload is composed with a tag-only object instead.
        (Some(tag), None) => match &variant.fields {
            Fields::Named(named) => {
                described(object_body(&named.named, &fields, Some((tag, read))))
            }
            Fields::Unit | Fields::Unnamed(_) => {
                // Never closed: a unit ignores the keys beside it, a payload reads them.
                let open = Container::default();
                let marker = object_body(&Punctuated::new(), &open, Some((tag, read)));
                match payload(&variant.fields, &fields) {
                    None => described(marker),
                    Some(payload) => described(quote! {
                        {
                            let mut keywords = ::kynos::openapi::SchemaObject::default();
                            keywords.all_of = ::core::option::Option::Some(
                                ::std::vec![#marker, #payload],
                            );
                            ::kynos::openapi::Schema::Object(::std::boxed::Box::new(keywords))
                        }
                    }),
                }
            }
        },

        // Externally tagged: one entry keyed by the variant's name, or a unit
        // variant's bare name.
        (None, _) => match payload(&variant.fields, &fields) {
            None => described(named_string(read)),
            Some(payload) => described(keyed(read, &payload)),
        },
    }
}

/// The schema of what a variant carries, or nothing for a unit-like variant.
pub(super) fn payload(fields: &Fields, container: &Container) -> Option<TokenStream2> {
    match fields {
        Fields::Unit => None,
        Fields::Named(named) => Some(object_body(&named.named, container, None)),
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            (!is_unit_like(fields)).then(|| member_schema(&unnamed.unnamed[0]))
        }
        // An enum carries no container default.
        Fields::Unnamed(unnamed) => Some(tuple_body(&unnamed.unnamed, false)),
    }
}
