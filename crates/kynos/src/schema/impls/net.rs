//! Addresses, which JSON Schema has named formats for.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{
    Schema,
    impls::{formatted, with_object},
    registry::Registry,
};

impl Schema for Ipv4Addr {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::String, "ipv4")
    }
}

impl Schema for Ipv6Addr {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::String, "ipv6")
    }
}

/// Either address family.
///
/// `format` names one family or the other, so a type that admits both is a
/// union rather than a third format.
impl Schema for IpAddr {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        let branches = vec![Ipv4Addr::schema(registry), Ipv6Addr::schema(registry)];
        with_object(OpenApiSchema::default(), |object| {
            object.any_of = Some(branches);
        })
    }
}
