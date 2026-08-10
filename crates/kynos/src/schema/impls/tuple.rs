//! Tuples, described as arrays of a fixed shape.
//!
//! `prefixItems` names each position, `items: false` forbids a longer array,
//! and `minItems` forbids a shorter one. All three are needed: `prefixItems`
//! alone constrains only the elements that are present.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{Schema, impls::with_object, registry::Registry};

/// Emits one implementation per arity.
macro_rules! tuples {
    ($(($($member:ident),+));+ $(;)?) => {
        $(
            impl<$($member: Schema),+> Schema for ($($member,)+) {
                fn schema(registry: &mut Registry) -> OpenApiSchema {
                    let prefix = vec![$(registry.resolve::<$member>()),+];
                    let length = prefix.len() as u64;
                    with_object(OpenApiSchema::of_type(SchemaType::Array), |object| {
                        object.prefix_items = Some(prefix);
                        object.items = Some(Box::new(OpenApiSchema::never()));
                        object.min_items = Some(length);
                    })
                }
            }
        )+
    };
}

tuples! {
    (A);
    (A, B);
    (A, B, C);
    (A, B, C, D);
    (A, B, C, D, E);
    (A, B, C, D, E, F);
    (A, B, C, D, E, F, G);
    (A, B, C, D, E, F, G, H);
    (A, B, C, D, E, F, G, H, I);
    (A, B, C, D, E, F, G, H, I, J);
    (A, B, C, D, E, F, G, H, I, J, K);
    (A, B, C, D, E, F, G, H, I, J, K, L);
}

/// The empty tuple, which serde writes as `null`.
///
/// This is what a handler returning nothing describes its body as, so it is not
/// merely an oddity for completeness.
impl Schema for () {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        OpenApiSchema::of_type(SchemaType::Null)
    }
}
