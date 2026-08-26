//! Booleans, numbers, and strings.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{
    Schema,
    impls::{formatted, integer, with_object},
    registry::Registry,
};

impl Schema for bool {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        OpenApiSchema::of_type(SchemaType::Boolean)
    }
}

impl Schema for String {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        OpenApiSchema::of_type(SchemaType::String)
    }
}

// The length bounds stay beside the format rather than being replaced by it.
// Registry support is optional, so a tool that does not know `char` still has
// to receive the constraint that makes the schema constraining.
impl Schema for char {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        with_object(formatted(SchemaType::String, "char"), |object| {
            object.min_length = Some(1);
            object.max_length = Some(1);
        })
    }
}

/// Emits the signed widths, with their exact ranges.
macro_rules! signed {
    ($($ty:ty => $format:literal),+ $(,)?) => {
        $(
            impl Schema for $ty {
                fn schema(_registry: &mut Registry) -> OpenApiSchema {
                    integer(
                        $format,
                        Some(f64::from(<$ty>::MIN)),
                        Some(f64::from(<$ty>::MAX)),
                    )
                }
            }
        )+
    };
}

/// Emits the unsigned widths, whose lower bound is always zero.
macro_rules! unsigned {
    ($($ty:ty => $format:literal),+ $(,)?) => {
        $(
            impl Schema for $ty {
                fn schema(_registry: &mut Registry) -> OpenApiSchema {
                    integer($format, Some(0.0), Some(f64::from(<$ty>::MAX)))
                }
            }
        )+
    };
}

// OAS defines only `int32` and `int64`, and both are signed. The OAI Format
// Registry names every width in both signednesses, so each type gets the format
// it actually is rather than the nearest one that can hold it — no `u32` widened
// to `int64` for want of a `uint32`.
signed!(i8 => "int8", i16 => "int16", i32 => "int32");
unsigned!(u8 => "uint8", u16 => "uint16", u32 => "uint32");

// `i64::MAX` and `u64::MAX` are not representable in an `f64`, and JSON Schema
// bounds are numbers. Emitting a rounded bound would forbid values the type
// accepts, or accept values it does not, so at this width the format carries
// what the bounds cannot. `u64` keeps `minimum: 0` because that one bound *is*
// exactly representable.
impl Schema for i64 {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::Integer, "int64")
    }
}

impl Schema for u64 {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        with_object(formatted(SchemaType::Integer, "uint64"), |object| {
            object.minimum = Some(0.0);
        })
    }
}

impl Schema for f32 {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::Number, "float")
    }
}

impl Schema for f64 {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::Number, "double")
    }
}
