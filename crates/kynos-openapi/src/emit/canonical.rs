//! [`Document::to_canonical_json`]: JSON whose bytes the document alone fixes.
//!
//! The document goes through a `serde_json::Value`, whose object order is the
//! build's (sorted, or insertion order under `serde_json/preserve_order`), so
//! the writer sorts each object itself rather than trusting it. A `Value` keeps
//! one value per key, so the duplicate check runs first, over the direct
//! serialization that still writes every key.

use std::{collections::HashSet, fmt};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;

use crate::model::document::Document;

impl Document {
    /// Serializes to canonical JSON: the same bytes for the same document,
    /// whatever the build.
    ///
    /// Every object's keys are sorted by Unicode code point, at every depth,
    /// including inside examples, defaults and extension values; arrays keep
    /// their order. The text is indented by two spaces, with `": "` after
    /// each key, and ends with one newline. Strings and numbers are written as
    /// [`to_json`](Self::to_json) writes them.
    ///
    /// Sorting changes no meaning. Neither OpenAPI version gives an object's
    /// key order a meaning: each construct whose order matters, such as
    /// `tags`, `servers`, `parameters` or `prefixItems`, is an array.
    ///
    /// Use this, rather than `to_json`, for a description committed beside the
    /// code: no `serde_json` feature a dependency enables changes it, and
    /// reordering mounts or fields changes nothing either, so the file changes
    /// only when the description does.
    ///
    /// # Errors
    ///
    /// Returns an error where [`to_json`](Self::to_json) would, and where one
    /// object would carry the same key twice, an object with no single
    /// canonical form. Only a map flattened beside an object's fields can
    /// name one of them again: an extension named like a field of its
    /// object, or an entry of a Schema Object's
    /// [`unknown_keywords`](crate::model::schema::object::SchemaObject::unknown_keywords)
    /// named like one of its typed keywords.
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        let direct = serde_json::to_string(self)?;
        DuplicateKeys::refuse_in(&direct)?;
        let value = serde_json::to_value(self)?;
        let mut canonical = serde_json::to_string_pretty(&Sorted(&value))?;
        canonical.push('\n');
        Ok(canonical)
    }
}

/// A `Value` serialized with each object's keys in sorted order.
struct Sorted<'a>(&'a Value);

impl Serialize for Sorted<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(object) => {
                let mut entries: Vec<_> = object.iter().collect();
                // Keys of one object are distinct, so stability is moot.
                entries.sort_unstable_by_key(|(one, _)| *one);
                serializer.collect_map(entries.into_iter().map(|(key, value)| (key, Sorted(value))))
            }
            Value::Array(items) => serializer.collect_seq(items.iter().map(Sorted)),
            scalar => scalar.serialize(serializer),
        }
    }
}

/// Reads JSON only to refuse an object carrying one key twice.
///
/// Under `serde_json/arbitrary_precision` a number reads as a one-key object,
/// which this passes as it passes any other.
struct DuplicateKeys;

impl DuplicateKeys {
    fn refuse_in(json: &str) -> Result<(), serde_json::Error> {
        serde_json::from_str::<Self>(json).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for DuplicateKeys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Self)
    }
}

impl<'de> Visitor<'de> for DuplicateKeys {
    type Value = Self;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_str<E>(self, _: &str) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_unit<E>(self) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Self, A::Error> {
        while items.next_element::<Self>()?.is_some() {}
        Ok(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Self, A::Error> {
        let mut seen = HashSet::new();
        while let Some(key) = entries.next_key::<String>()? {
            entries.next_value::<Self>()?;
            if seen.contains(&key) {
                return Err(de::Error::custom(format_args!(
                    "the key `{key}` appears twice in one object"
                )));
            }
            seen.insert(key);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
