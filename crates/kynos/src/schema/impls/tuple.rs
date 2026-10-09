//! Tuples, described as arrays of a fixed shape.
//!
//! `prefixItems` names each position, `items: false` forbids a longer array,
//! and `minItems` forbids a shorter one. All three are needed: `prefixItems`
//! alone constrains only the elements that are present.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{
    Schema,
    constraints::{Pointer, Violations},
    impls::with_object,
    registry::Registry,
};

/// Emits one implementation per arity.
macro_rules! tuples {
    ($(($($member:ident),+));+ $(;)?) => {
        $(
            impl<$($member: Schema),+> Schema for ($($member,)+) {
                fn schema(registry: &mut Registry) -> OpenApiSchema {
                    let prefix = vec![$(registry.resolve::<$member>()),+];
                    let length = u64::try_from(prefix.len()).unwrap_or(u64::MAX);
                    with_object(OpenApiSchema::of_type(SchemaType::Array), |object| {
                        object.prefix_items = Some(prefix);
                        object.items = Some(Box::new(OpenApiSchema::never()));
                        object.min_items = Some(length);
                    })
                }

                // Members are bound under their type parameters' names.
                #[allow(non_snake_case)]
                fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
                    let ($($member,)+) = self;
                    let mut index = 0;
                    $(
                        $member.check_constraints(at.index(index), violations);
                        index += 1;
                    )+
                    let _ = index;
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
impl Schema for () {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        OpenApiSchema::of_type(SchemaType::Null)
    }
}
