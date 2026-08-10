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

impl Schema for char {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        with_object(OpenApiSchema::of_type(SchemaType::String), |object| {
            object.min_length = Some(1);
            object.max_length = Some(1);
        })
    }
}

/// Emits the two signed widths OAS names, with their exact ranges.
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

signed!(i8 => "int32", i16 => "int32", i32 => "int32");

// `int32` and `int64` are *signed* in the OAS Format Registry, so a `u32` does
// not fit `int32` — a generator honouring the format would emit a type its own
// maximum overflows. The bounds stay exact; the format widens.
unsigned!(u8 => "int32", u16 => "int32", u32 => "int64");

// `i64::MAX` and `u64::MAX` are not representable in an `f64`, and JSON Schema
// bounds are numbers. Emitting a rounded bound would forbid values the type
// accepts, or accept values it does not, so the width is left to the format —
// which is the only honest thing the vocabulary can say here. `u64` keeps
// `int64` for the same reason it keeps no maximum: nothing narrower is true,
// and its `minimum: 0` is.
impl Schema for i64 {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        formatted(SchemaType::Integer, "int64")
    }
}

impl Schema for u64 {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        with_object(formatted(SchemaType::Integer, "int64"), |object| {
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
