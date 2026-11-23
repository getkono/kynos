//! The serializers [`leading_member`](super::leading_member) reads a
//! payload's first member with.
//!
//! Private: they serve only that function, and change only with serde's data
//! model, never with the event framing beside them.

use serde::{
    Serialize,
    ser::{Impossible, SerializeMap, SerializeStruct, Serializer},
};

/// Ends every serialization: the member was found, or there is none.
#[derive(Debug)]
pub(super) struct Stop;

impl std::fmt::Display for Stop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the leading member was read")
    }
}

impl std::error::Error for Stop {}

impl serde::ser::Error for Stop {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Self
    }
}

/// Refuses each listed serializer method with [`Stop`].
macro_rules! refuse {
    ($($method:ident($($argument:ty),*) -> $ok:ty;)*) => {
        $(
            fn $method(self, $(_: $argument),*) -> Result<$ok, Stop> {
                Err(Stop)
            }
        )*
    };
}

/// The payload itself: an object opens, anything else is refused.
pub(super) struct Leading<'a> {
    pub(super) found: &'a mut Option<(String, String)>,
}

impl<'a> Serializer for Leading<'a> {
    type Ok = ();
    type Error = Stop;
    type SerializeSeq = Impossible<(), Stop>;
    type SerializeTuple = Impossible<(), Stop>;
    type SerializeTupleStruct = Impossible<(), Stop>;
    type SerializeTupleVariant = Impossible<(), Stop>;
    type SerializeMap = Map<'a>;
    type SerializeStruct = Struct<'a>;
    type SerializeStructVariant = Impossible<(), Stop>;

    refuse! {
        serialize_bool(bool) -> ();
        serialize_i8(i8) -> ();
        serialize_i16(i16) -> ();
        serialize_i32(i32) -> ();
        serialize_i64(i64) -> ();
        serialize_i128(i128) -> ();
        serialize_u8(u8) -> ();
        serialize_u16(u16) -> ();
        serialize_u32(u32) -> ();
        serialize_u64(u64) -> ();
        serialize_u128(u128) -> ();
        serialize_f32(f32) -> ();
        serialize_f64(f64) -> ();
        serialize_char(char) -> ();
        serialize_str(&str) -> ();
        serialize_bytes(&[u8]) -> ();
        serialize_none() -> ();
        serialize_unit() -> ();
        serialize_unit_struct(&'static str) -> ();
        serialize_unit_variant(&'static str, u32, &'static str) -> ();
        serialize_seq(Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(usize) -> Self::SerializeTuple;
        serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
        serialize_tuple_variant(&'static str, u32, &'static str, usize)
            -> Self::SerializeTupleVariant;
        serialize_struct_variant(&'static str, u32, &'static str, usize)
            -> Self::SerializeStructVariant;
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<(), Stop> {
        Err(Stop)
    }

    // A newtype struct is written as what it wraps, so its tag is too.
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Stop> {
        value.serialize(self)
    }

    // An externally tagged variant: its name is a key, not a member.
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<(), Stop> {
        Err(Stop)
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Map<'a>, Stop> {
        Ok(Map {
            found: self.found,
            key: None,
        })
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Struct<'a>, Stop> {
        Ok(Struct { found: self.found })
    }
}

/// An object written as a struct, whose first field is the member.
pub(super) struct Struct<'a> {
    found: &'a mut Option<(String, String)>,
}

impl SerializeStruct for Struct<'_> {
    type Ok = ();
    type Error = Stop;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Stop> {
        *self.found = Some((key.to_owned(), value.serialize(Text)?));
        Err(Stop)
    }

    fn end(self) -> Result<(), Stop> {
        Err(Stop)
    }
}

/// An object written as a map, whose first entry is the member.
pub(super) struct Map<'a> {
    found: &'a mut Option<(String, String)>,
    key: Option<String>,
}

impl SerializeMap for Map<'_> {
    type Ok = ();
    type Error = Stop;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Stop> {
        self.key = Some(key.serialize(Text)?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Stop> {
        let key = self.key.take().ok_or(Stop)?;
        *self.found = Some((key, value.serialize(Text)?));
        Err(Stop)
    }

    fn end(self) -> Result<(), Stop> {
        Err(Stop)
    }
}

/// A member's key or value, which is read only as a string.
struct Text;

impl Serializer for Text {
    type Ok = String;
    type Error = Stop;
    type SerializeSeq = Impossible<String, Stop>;
    type SerializeTuple = Impossible<String, Stop>;
    type SerializeTupleStruct = Impossible<String, Stop>;
    type SerializeTupleVariant = Impossible<String, Stop>;
    type SerializeMap = Impossible<String, Stop>;
    type SerializeStruct = Impossible<String, Stop>;
    type SerializeStructVariant = Impossible<String, Stop>;

    refuse! {
        serialize_bool(bool) -> String;
        serialize_i8(i8) -> String;
        serialize_i16(i16) -> String;
        serialize_i32(i32) -> String;
        serialize_i64(i64) -> String;
        serialize_i128(i128) -> String;
        serialize_u8(u8) -> String;
        serialize_u16(u16) -> String;
        serialize_u32(u32) -> String;
        serialize_u64(u64) -> String;
        serialize_u128(u128) -> String;
        serialize_f32(f32) -> String;
        serialize_f64(f64) -> String;
        serialize_char(char) -> String;
        serialize_bytes(&[u8]) -> String;
        serialize_none() -> String;
        serialize_unit() -> String;
        serialize_unit_struct(&'static str) -> String;
        serialize_seq(Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(usize) -> Self::SerializeTuple;
        serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
        serialize_tuple_variant(&'static str, u32, &'static str, usize)
            -> Self::SerializeTupleVariant;
        serialize_map(Option<usize>) -> Self::SerializeMap;
        serialize_struct(&'static str, usize) -> Self::SerializeStruct;
        serialize_struct_variant(&'static str, u32, &'static str, usize)
            -> Self::SerializeStructVariant;
    }

    fn serialize_str(self, text: &str) -> Result<String, Stop> {
        Ok(text.to_owned())
    }

    // An adjacent tag is written as a unit variant, which JSON spells as its
    // name.
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String, Stop> {
        Ok(variant.to_owned())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String, Stop> {
        Err(Stop)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<String, Stop> {
        Err(Stop)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, Stop> {
        Err(Stop)
    }
}
