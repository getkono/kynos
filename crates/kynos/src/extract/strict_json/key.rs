//! Naming the member a key reads, so a failure in its value is keyed under it.
//!
//! A key is read by its own seed, whose value is opaque here, so the name is
//! taken from what the seed's visitor is handed on its way through.

use std::{cell::RefCell, fmt, ops::Range};

use serde::de::{DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, Visitor};

/// A member's name, as its key was read.
#[derive(Debug, Default)]
pub(super) enum Key<'de> {
    /// Nothing named yet.
    #[default]
    Absent,

    /// Borrowed from the document, as every unescaped key is, so naming it
    /// costs nothing.
    Borrowed(&'de str),

    /// Where the name sits in the document's key buffer: an escaped key,
    /// which `serde_json` unescapes into scratch it reuses for the next one,
    /// or a key read as a number or a character, written as the value read.
    Buffered(Range<usize>),
}

impl Key<'_> {
    /// The name, owned for a failure's path; `None` where none was read.
    pub(super) fn name(&self, keys: &RefCell<String>) -> Option<String> {
        match self {
            Self::Absent => None,
            Self::Borrowed(name) => Some((*name).to_owned()),
            Self::Buffered(range) => keys.borrow().get(range.clone()).map(str::to_owned),
        }
    }
}

/// A key's seed, deserializer, visitor or enum access, recording into `slot`
/// the name its visitor is handed.
///
/// A name that cannot be borrowed is written to `keys`, which every key of a
/// document shares, so a document's escaped keys allocate once between them
/// rather than once each.
pub(super) struct Naming<'s, 'de, X> {
    /// What every request is forwarded to.
    pub(super) inner: X,

    /// Where the name read is recorded.
    pub(super) slot: &'s mut Key<'de>,

    /// The document's key buffer.
    pub(super) keys: &'s RefCell<String>,
}

impl<'s, 'de, X> Naming<'s, 'de, X> {
    /// The same slot and buffer, around `inner`.
    fn around<Y>(self, inner: Y) -> (X, Naming<'s, 'de, Y>) {
        let Self {
            inner: own,
            slot,
            keys,
        } = self;
        (own, Naming { inner, slot, keys })
    }

    /// Records `name` as written, in the key buffer.
    fn write(&mut self, name: impl fmt::Display) {
        use std::fmt::Write as _;

        let mut keys = self.keys.borrow_mut();
        let start = keys.len();
        // Writing to a `String` cannot fail.
        let _ = write!(keys, "{name}");
        *self.slot = Key::Buffered(start..keys.len());
    }
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Naming<'_, 'de, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        let (seed, deserializer) = self.around(deserializer);
        seed.deserialize(deserializer)
    }
}

/// Forwards each named `deserialize_*` method, with its extra arguments, to
/// the inner deserializer, the visitor wrapped to name what it is handed.
macro_rules! forward_naming {
    ($($method:ident($($argument:ident: $kind:ty),*);)*) => {$(
        fn $method<V: Visitor<'de>>(
            self,
            $($argument: $kind,)*
            visitor: V,
        ) -> Result<V::Value, Self::Error> {
            let (inner, visitor) = self.around(visitor);
            inner.$method($($argument,)* visitor)
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Naming<'_, 'de, D> {
    type Error = D::Error;

    forward_naming! {
        deserialize_any();
        deserialize_bool();
        deserialize_i8();
        deserialize_i16();
        deserialize_i32();
        deserialize_i64();
        deserialize_i128();
        deserialize_u8();
        deserialize_u16();
        deserialize_u32();
        deserialize_u64();
        deserialize_u128();
        deserialize_f32();
        deserialize_f64();
        deserialize_char();
        deserialize_str();
        deserialize_string();
        deserialize_bytes();
        deserialize_byte_buf();
        deserialize_option();
        deserialize_unit();
        deserialize_unit_struct(name: &'static str);
        deserialize_newtype_struct(name: &'static str);
        deserialize_seq();
        deserialize_tuple(len: usize);
        deserialize_tuple_struct(name: &'static str, len: usize);
        deserialize_map();
        deserialize_struct(name: &'static str, fields: &'static [&'static str]);
        deserialize_enum(name: &'static str, variants: &'static [&'static str]);
        deserialize_identifier();
        deserialize_ignored_any();
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

/// Forwards each named `visit_*` method to the inner visitor, after writing
/// the value it is handed as the key's name.
macro_rules! visit_written {
    ($($method:ident($kind:ty);)*) => {$(
        fn $method<E: serde::de::Error>(mut self, value: $kind) -> Result<Self::Value, E> {
            self.write(&value);
            self.inner.$method(value)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Naming<'_, 'de, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(formatter)
    }

    visit_written! {
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
        visit_string(String);
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
        *self.slot = Key::Borrowed(if value { "true" } else { "false" });
        self.inner.visit_bool(value)
    }

    fn visit_borrowed_str<E: serde::de::Error>(self, value: &'de str) -> Result<Self::Value, E> {
        *self.slot = Key::Borrowed(value);
        self.inner.visit_borrowed_str(value)
    }

    fn visit_bytes<E: serde::de::Error>(mut self, value: &[u8]) -> Result<Self::Value, E> {
        self.write(String::from_utf8_lossy(value));
        self.inner.visit_bytes(value)
    }

    fn visit_borrowed_bytes<E: serde::de::Error>(
        mut self,
        value: &'de [u8],
    ) -> Result<Self::Value, E> {
        match std::str::from_utf8(value) {
            Ok(name) => *self.slot = Key::Borrowed(name),
            Err(_) => self.write(String::from_utf8_lossy(value)),
        }
        self.inner.visit_borrowed_bytes(value)
    }

    fn visit_byte_buf<E: serde::de::Error>(mut self, value: Vec<u8>) -> Result<Self::Value, E> {
        self.write(String::from_utf8_lossy(&value));
        self.inner.visit_byte_buf(value)
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        let (visitor, deserializer) = self.around(deserializer);
        visitor.visit_some(deserializer)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        let (visitor, deserializer) = self.around(deserializer);
        visitor.visit_newtype_struct(deserializer)
    }

    /// No JSON key is a sequence or a map; one read as either names nothing.
    fn visit_seq<A: SeqAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_seq(access)
    }

    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_map(access)
    }

    /// An enum key is named by its variant.
    fn visit_enum<A: EnumAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        let (visitor, access) = self.around(access);
        visitor.visit_enum(access)
    }
}

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Naming<'_, 'de, A> {
    type Error = A::Error;
    type Variant = A::Variant;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let (access, seed) = self.around(seed);
        access.variant_seed(seed)
    }
}
