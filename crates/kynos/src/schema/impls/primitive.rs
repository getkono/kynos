//! Booleans, numbers, and strings.

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{
    ParamValue, Schema,
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

// Length bounds beside the format, since registry format support is optional.
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

// The OAI Format Registry names every width and signedness, so each type gets
// its exact format.
signed!(i8 => "int8", i16 => "int16", i32 => "int32");
unsigned!(u8 => "uint8", u16 => "uint16", u32 => "uint32");

// `i64::MAX` and `u64::MAX` round in an `f64`, so the format carries what the
// bounds cannot; `u64` keeps `minimum: 0`, which is exact.
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

// `Display` writes what the schema describes, but for the non-finite floats
// `ParamValue` lists as an accepted exception.
impl ParamValue for bool {}
impl ParamValue for char {}
impl ParamValue for String {}
impl ParamValue for i8 {}
impl ParamValue for i16 {}
impl ParamValue for i32 {}
impl ParamValue for i64 {}
impl ParamValue for u8 {}
impl ParamValue for u16 {}
impl ParamValue for u32 {}
impl ParamValue for u64 {}
impl ParamValue for f32 {}
impl ParamValue for f64 {}
