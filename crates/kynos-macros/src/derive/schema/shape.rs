use super::{
    Comma, Container, DataEnum, Field, Fields, Punctuated, TokenStream2, Variant,
    aliases::{keyed, named_string, property, variant_names},
    closed, constraints, deprecate, described, described_variants, doc_string, is_deprecated,
    is_described, is_flattened, is_open, is_phantom, is_unit_like, min_items, positional_members,
    quote, transparent_member,
};

/// A struct's schema, which its fields decide.
///
/// A `#[serde(transparent)]` struct is the schema of its transparent field,
/// whichever shape declares it: the field serde both writes and reads through,
/// or the single field of the one direction serde can derive, because that
/// field's value is all the wire carries. `reject_transparent_without_one_field`
/// refuses the struct where the two directions pick different fields, and reads
/// the same picks, so no field described here is one that refusal refused.
///
/// A newtype is transparent, because serde makes it so: `Sku(String)` is a
/// string on the wire, under what its member declares, and describing it as
/// anything else would be a claim the serializer contradicts. A longer tuple is
/// the array serde writes, and a unit struct is `null`.
pub(super) fn struct_body(fields: &Fields, container: &Container) -> TokenStream2 {
    if let (true, Some(field)) = (container.transparent, transparent_member(fields)) {
        return member_schema(field);
    }

    match fields {
        Fields::Named(named) => object_body(&named.named, container, None),
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
/// `tag` is `(property, names)` for an internally tagged enum variant, which is
/// an object whose fields are the variant's plus the one that says which
/// variant it is, under any name serde reads it by. Closed as
/// [`closed`](super::closed) says.
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
                    // `#[schema(open)]` is the declaration that this object
                    // really is open, which is the only thing a flattened map
                    // can be: it names no member, so its value schema has to
                    // reach every member nothing else described.
                    //
                    // `additionalProperties` cannot say that from inside an
                    // `allOf` branch -- it is defined against the `properties`
                    // of its own schema object, and there are none there, so it
                    // would apply to the members this object declared itself.
                    // `unevaluatedProperties` is the one keyword that sees
                    // annotations across `allOf`, so it lands on the parent.
                    return quote! {
                        {
                            let mut flattened = registry.resolve::<#ty>();
                            if let ::kynos::openapi::Schema::Object(open) = &mut flattened {
                                keywords.unevaluated_properties =
                                    open.additional_properties.take();
                                // A key constraint has nowhere to go. Inside the
                                // branch `propertyNames` names this object's own
                                // properties too, and `patternProperties` is not
                                // emitted -- so it is dropped, leaving a schema
                                // weaker than the type rather than one that
                                // contradicts it. `docs/schema.md` records it.
                                open.property_names = ::core::option::Option::None;
                            }
                            keywords
                                .all_of
                                .get_or_insert_with(::std::vec::Vec::new)
                                .push(flattened);
                        }
                    };
                }

                // A flattened field's properties belong to this object, and which
                // ones they are is only known once its own schema is built. `allOf`
                // is the composition that says so without naming them.
                //
                // Which is sound only because the field's type is
                // `kynos::schema::Flatten`, asserted by the witness
                // `flatten_witnesses` emits: a schema constraining members it
                // does not name would reach this object's own properties from
                // inside the branch.
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
/// The prose sits beside the schema, which for a named field type is a `$ref`
/// -- legal from 3.1 onward, where a schema `$ref` applies its siblings. A
/// boolean schema has nowhere to put it and keeps none.
///
/// A `PhantomData` is resolved as `()`, the `null` serde writes and reads for
/// both, since `PhantomData<T>: Schema` is a bound nothing satisfies.
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

/// An enum's schema, which its tagging decides.
///
/// Four shapes, and which applies is read from the serde attributes rather than
/// chosen here: an enumeration of names where every variant is a unit, and
/// otherwise the `oneOf` that matches how the payload is tagged. A variant serde
/// reads and never writes is described as serde reads it.
pub(super) fn enum_body(data: &DataEnum, container: &Container) -> TokenStream2 {
    let variants = described_variants(data);

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
        && variants.iter().all(|variant| is_unit_like(&variant.fields))
    {
        // Every name serde reads, each once: `enum` items should be unique.
        let mut names: Vec<String> = Vec::new();
        for name in variants
            .iter()
            .flat_map(|variant| variant_names(variant, container))
        {
            if !names.contains(&name) {
                names.push(name);
            }
        }
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
    // It maps no value: every branch is inline, which implicit mapping does not
    // consider and no mapping value names, so each tag value, an alias
    // included, reaches its branch through the tag property's own `const` or
    // `enum`.
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
/// every name serde reads it by.
pub(super) fn branch(variant: &Variant, container: &Container) -> TokenStream2 {
    let read = variant_names(variant, container);
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
            let tagged = named_string(&read);
            let payload = payload(&variant.fields, container).map(|payload| {
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

        // Internally tagged: the tag is one more property of the variant's own
        // object. A newtype variant has no properties of its own to add it to,
        // so the two are composed instead, unless serde writes it as a unit.
        (Some(tag), None) => match &variant.fields {
            Fields::Named(named) => {
                described(object_body(&named.named, container, Some((tag, &read))))
            }
            Fields::Unit | Fields::Unnamed(_) => {
                // Never closed: a unit ignores the keys beside it, a payload reads them.
                let open = Container::default();
                let marker = object_body(&Punctuated::new(), &open, Some((tag, &read)));
                match payload(&variant.fields, container) {
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

        // Externally tagged: a name of the variant's is the one entry serde
        // reads, so nothing beside it, and a unit variant is that name as a
        // bare string.
        (None, _) => match payload(&variant.fields, container) {
            None => described(named_string(&read)),
            Some(payload) => described(keyed(&read, &payload)),
        },
    }
}

/// The schema of what a variant carries, a newtype variant's under what its
/// member declares, or nothing for a unit variant, which a newtype variant
/// whose member serde skips is on the wire.
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
