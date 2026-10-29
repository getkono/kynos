//! The accessors a container hands its members and elements out through, each
//! naming the step a failure inside one unwinds through.

use std::cell::Cell;

use serde::de::{
    DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};

use super::{
    State, Step, Strict, Visit,
    key::{Key, Naming},
};

/// A sequence's elements, each read through [`Strict`] and a failure in one
/// keyed at its index.
pub(super) struct Elements<'a, A> {
    pub(super) inner: A,
    pub(super) state: &'a State,

    /// Where the key buffer stood when the sequence began.
    pub(super) start: usize,

    /// The next element's index.
    pub(super) index: usize,
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Elements<'_, A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        inner: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        self.state.keys.borrow_mut().truncate(self.start);
        let state = self.state;
        state.begin();
        let index = self.index;
        let result = self.inner.next_element_seed(Strict { inner, state });
        // An element that failed was still read, so the next is the next one.
        if !matches!(result, Ok(None)) {
            self.index += 1;
        }
        state.at(result, || Some(Step::Index(index)))
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// An object's members, each value read through [`Strict`] and a failure in
/// one keyed at its name; a key is a string, never a struct.
pub(super) struct Members<'a, 'de, A> {
    pub(super) inner: A,
    pub(super) state: &'a State,

    /// Where the key buffer stood when the object began.
    pub(super) start: usize,

    /// The member being read.
    pub(super) key: Key<'de>,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Members<'_, 'de, A> {
    type Error = A::Error;

    /// A key that does not read, such as one `deny_unknown_fields` refuses,
    /// is keyed at its own member.
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        let state = self.state;
        state.begin();
        state.keys.borrow_mut().truncate(self.start);
        self.key = Key::Absent;
        let result = self.inner.next_key_seed(Naming {
            inner: seed,
            slot: &mut self.key,
            keys: &state.keys,
        });
        state.at(result, || self.key.name(&state.keys).map(Step::Member))
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(
        &mut self,
        inner: T,
    ) -> Result<T::Value, Self::Error> {
        let state = self.state;
        state.begin();
        let result = self.inner.next_value_seed(Strict { inner, state });
        state.at(result, || self.key.name(&state.keys).map(Step::Member))
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// The variant's content read through [`Strict`]; its name is an identifier,
/// and a variant it does not name is the enum's failure, since `serde_json`
/// reads a bare string and an object's key alike.
impl<'a, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Strict<'a, A> {
    type Error = A::Error;
    type Variant = Variant<'a, 'de, A::Variant>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let state = self.state;
        state.begin();
        let mut name = Key::Absent;
        let (value, inner) = self.inner.variant_seed(Naming {
            inner: seed,
            slot: &mut name,
            keys: &state.keys,
        })?;
        Ok((value, Variant { inner, state, name }))
    }
}

/// A variant's content, a failure inside it keyed under the variant's name.
///
/// Content that is not the kind its variant declares is the enum's failure,
/// keyed at the enum: `serde_json` reports it as it reports a variant
/// holding content written as a bare string, which names no member. A newtype
/// variant's content is reached only when the document has some.
pub(super) struct Variant<'a, 'de, A> {
    inner: A,
    state: &'a State,

    /// The variant's name, as the document wrote it.
    name: Key<'de>,
}

impl State {
    /// Passes a variant's `result` on, keyed under its `name` once `entered`
    /// is set.
    fn within<T, E>(
        &self,
        name: &Key<'_>,
        entered: &Cell<bool>,
        result: Result<T, E>,
    ) -> Result<T, E> {
        self.at(result, || {
            if entered.get() {
                name.name(&self.keys).map(Step::Member)
            } else {
                None
            }
        })
    }
}

/// A struct variant is read only from an object, as a struct is.
impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Variant<'_, 'de, A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        self.inner.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        inner: T,
    ) -> Result<T::Value, Self::Error> {
        self.state.begin();
        let entered = Cell::new(false);
        let seed = Entering {
            inner: Strict {
                inner,
                state: self.state,
            },
            entered: &entered,
        };
        let result = self.inner.newtype_variant_seed(seed);
        self.state.within(&self.name, &entered, result)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.state.begin();
        let entered = Cell::new(false);
        let visitor = Visit::new(visitor, self.state, false, Some(&entered));
        let result = self.inner.tuple_variant(len, visitor);
        self.state.within(&self.name, &entered, result)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.state.begin();
        let entered = Cell::new(false);
        let visitor = Visit::new(visitor, self.state, true, Some(&entered));
        let result = self.inner.struct_variant(fields, visitor);
        self.state.within(&self.name, &entered, result)
    }
}

/// A newtype variant's seed, noting that the content was reached.
struct Entering<'c, S> {
    inner: S,
    entered: &'c Cell<bool>,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Entering<'_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.entered.set(true);
        self.inner.deserialize(deserializer)
    }
}
