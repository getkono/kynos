//! Reading a struct only from an object, at every position of a document.

use std::cell::Cell;

use serde::de::{
    DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};

/// A position in a document, refusing to read a struct from an array, and
/// handing the same refusal to every position it hands out: as the
/// deserializer, seed, element and variant accessors `serde_json` passes in.
///
/// Each wrapper adds a pointer and allocates nothing; what one costs per
/// element is the forwarding the compiler inlines.
pub(super) struct Strict<'a, D> {
    /// What every request is forwarded to.
    pub(super) inner: D,

    /// Set when an array was refused, so a syntax error later in the
    /// document can still win.
    pub(super) refused: &'a Cell<bool>,
}

/// Forwards each named `deserialize_*` method, with its extra arguments, to
/// the inner deserializer, the visitor wrapped where the method says `wrap`.
macro_rules! forward {
    ($($method:ident($($argument:ident: $kind:ty),*) $wrap:tt;)*) => {$(
        fn $method<V: Visitor<'de>>(
            self,
            $($argument: $kind,)*
            visitor: V,
        ) -> Result<V::Value, Self::Error> {
            let visitor = forward!(@visitor $wrap visitor self.refused);
            self.inner.$method($($argument,)* visitor)
        }
    )*};
    (@visitor raw $visitor:ident $refused:expr) => { $visitor };
    (@visitor wrap $visitor:ident $refused:expr) => {
        Visit { visitor: $visitor, refused: $refused, object: false }
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Strict<'_, D> {
    type Error = D::Error;

    forward! {
        deserialize_any() wrap;
        deserialize_bool() raw;
        deserialize_i8() raw;
        deserialize_i16() raw;
        deserialize_i32() raw;
        deserialize_i64() raw;
        deserialize_i128() raw;
        deserialize_u8() raw;
        deserialize_u16() raw;
        deserialize_u32() raw;
        deserialize_u64() raw;
        deserialize_u128() raw;
        deserialize_f32() raw;
        deserialize_f64() raw;
        deserialize_char() raw;
        deserialize_str() raw;
        deserialize_string() raw;
        deserialize_bytes() raw;
        deserialize_byte_buf() raw;
        deserialize_option() wrap;
        deserialize_unit() raw;
        deserialize_unit_struct(name: &'static str) raw;
        deserialize_seq() wrap;
        deserialize_tuple(len: usize) wrap;
        deserialize_tuple_struct(name: &'static str, len: usize) wrap;
        deserialize_map() wrap;
        deserialize_enum(name: &'static str, variants: &'static [&'static str]) wrap;
        deserialize_identifier() raw;
        deserialize_ignored_any() raw;
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = Visit {
            visitor,
            refused: self.refused,
            object: true,
        };
        self.inner.deserialize_struct(name, fields, visitor)
    }

    /// A newtype is its member, as `serde_json` reads it; `serde_json`'s own
    /// private tokens, such as `RawValue`'s, are its to read.
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name.starts_with("$serde_json::") {
            self.inner.deserialize_newtype_struct(name, visitor)
        } else {
            let visitor = Visit {
                visitor,
                refused: self.refused,
                object: false,
            };
            self.inner.deserialize_newtype_struct(name, visitor)
        }
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

/// A visitor whose nested positions are [`Strict`], refusing a sequence where
/// it was asked for a struct.
struct Visit<'a, V> {
    visitor: V,
    refused: &'a Cell<bool>,
    /// Whether the value asked for is a struct, which an array is not.
    object: bool,
}

/// Forwards each named scalar `visit_*` method to the inner visitor.
macro_rules! visit {
    ($($method:ident($kind:ty);)*) => {$(
        fn $method<E: serde::de::Error>(self, value: $kind) -> Result<Self::Value, E> {
            self.visitor.$method(value)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Visit<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.visitor.expecting(formatter)
    }

    visit! {
        visit_bool(bool);
        visit_i8(i8);
        visit_i16(i16);
        visit_i32(i32);
        visit_i64(i64);
        visit_i128(i128);
        visit_u8(u8);
        visit_u16(u16);
        visit_u32(u32);
        visit_u64(u64);
        visit_u128(u128);
        visit_f32(f32);
        visit_f64(f64);
        visit_char(char);
        visit_str(&str);
        visit_borrowed_str(&'de str);
        visit_string(String);
        visit_bytes(&[u8]);
        visit_borrowed_bytes(&'de [u8]);
        visit_byte_buf(Vec<u8>);
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_none()
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        let refused = self.refused;
        self.visitor.visit_some(Strict { inner, refused })
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        let refused = self.refused;
        self.visitor.visit_newtype_struct(Strict { inner, refused })
    }

    /// Refused before any element is read.
    fn visit_seq<A: SeqAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        if self.object {
            self.refused.set(true);
            return Err(serde::de::Error::invalid_type(
                serde::de::Unexpected::Seq,
                &"a JSON object",
            ));
        }
        let refused = self.refused;
        self.visitor.visit_seq(Strict { inner, refused })
    }

    fn visit_map<A: MapAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        let refused = self.refused;
        self.visitor.visit_map(Strict { inner, refused })
    }

    fn visit_enum<A: EnumAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        let refused = self.refused;
        self.visitor.visit_enum(Strict { inner, refused })
    }
}

/// A value read through [`Strict`].
impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Strict<'_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        let refused = self.refused;
        self.inner.deserialize(Strict { inner, refused })
    }
}

/// Each element read through [`Strict`].
impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Strict<'_, A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        inner: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        let refused = self.refused;
        self.inner.next_element_seed(Strict { inner, refused })
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// Each value read through [`Strict`]; a key is a string, never a struct.
impl<'de, A: MapAccess<'de>> MapAccess<'de> for Strict<'_, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        self.inner.next_key_seed(seed)
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(
        &mut self,
        inner: T,
    ) -> Result<T::Value, Self::Error> {
        let refused = self.refused;
        self.inner.next_value_seed(Strict { inner, refused })
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// The variant's content read through [`Strict`]; its name is an identifier.
impl<'a, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Strict<'a, A> {
    type Error = A::Error;
    type Variant = Strict<'a, A::Variant>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let refused = self.refused;
        let (value, inner) = self.inner.variant_seed(seed)?;
        Ok((value, Strict { inner, refused }))
    }
}

/// A struct variant is read only from an object, as a struct is.
impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Strict<'_, A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        self.inner.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        inner: T,
    ) -> Result<T::Value, Self::Error> {
        let refused = self.refused;
        self.inner.newtype_variant_seed(Strict { inner, refused })
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = Visit {
            visitor,
            refused: self.refused,
            object: false,
        };
        self.inner.tuple_variant(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = Visit {
            visitor,
            refused: self.refused,
            object: true,
        };
        self.inner.struct_variant(fields, visitor)
    }
}
