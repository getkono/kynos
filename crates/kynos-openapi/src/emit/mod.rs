//! Producing an artifact from a [`Document`], at a chosen specification
//! version.
//!
//! The split from [`crate::model`] is the one the architecture asks for: the
//! model is version-agnostic data, and everything that turns it into bytes at a
//! particular version lives here. The serde derives stay on the model types
//! themselves — they are part of how those types are represented, not part of
//! choosing a version to represent them at.

pub mod downgrade;

use crate::{
    model::document::{Document, SpecVersion},
    validate::violation::SpecError,
};

impl Document {
    /// Serializes to pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns an error only if a specification extension holds a value that
    /// cannot be represented in JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serializes to YAML.
    ///
    /// # Errors
    ///
    /// Returns an error only if a number is beyond the range of a 64-bit
    /// float. That can happen only when `serde_json`'s `arbitrary_precision`
    /// feature is unified into the build: without it, `serde_json` holds no
    /// such number to begin with.
    #[cfg(feature = "yaml")]
    pub fn to_yaml(&self) -> Result<String, serde_yaml_ng::Error> {
        let mut value = serde_yaml_ng::to_value(self)?;
        if yaml_numbers::serialized_as_token() {
            yaml_numbers::restore(&mut value)?;
        }
        serde_yaml_ng::to_string(&value)
    }

    /// Produces this document as `version`, refusing a lossy downgrade.
    ///
    /// Cargo unifies features across a dependency graph, so a program can find
    /// itself built with `openapi32` enabled even when it needs to publish a
    /// 3.1 description. This is the safe way to ask for one: rather than
    /// dropping 3.2-only constructs and emitting something that misdescribes
    /// the API, it fails and names what stands in the way.
    ///
    /// # Errors
    ///
    /// Returns [`SpecError::RequiresV3_2`] when the document uses a construct
    /// that `version` cannot express.
    pub fn emit(&self, version: SpecVersion) -> Result<Self, SpecError> {
        let blockers = downgrade::three_two_only_constructs(self);
        if !version.supports_3_2() && !blockers.is_empty() {
            return Err(SpecError::RequiresV3_2 { blockers });
        }

        let mut emitted = self.clone();
        version.as_str().clone_into(&mut emitted.openapi);
        Ok(emitted)
    }
}

/// Numbers as YAML writes them, whatever `serde_json` features the build
/// unifies.
///
/// With `serde_json/arbitrary_precision` on anywhere in the graph, a
/// `serde_json::Number` serializes as a one-field struct named by a private
/// token, holding its digits as a string. `serde_json`'s own serializer
/// recognises the token and `serde_yaml_ng`'s writes a mapping. Cargo unifies
/// the feature across the whole build and a crate cannot `cfg` on a
/// dependency's features, so this reads the serialized tree instead: it finds
/// each token mapping and writes the number back in its place.
#[cfg(feature = "yaml")]
mod yaml_numbers {
    use serde::ser::Error as _;
    use serde_yaml_ng::{Mapping, Number, Value};

    /// The name `serde_json` serializes a number under with
    /// `arbitrary_precision` on: `TOKEN` in its `number.rs`, which is
    /// `pub(crate)` and so cannot be named from here.
    /// `mise run test:arbitrary-precision` holds this copy to it.
    const TOKEN: &str = "$serde_json::private::Number";

    /// Whether this build serializes a `serde_json::Number` as a token
    /// mapping, which is whether `arbitrary_precision` is on.
    pub(super) fn serialized_as_token() -> bool {
        matches!(
            serde_yaml_ng::to_value(serde_json::Number::from(0u8)),
            Ok(Value::Mapping(_))
        )
    }

    /// Replaces every token mapping in `value` with the number it holds.
    pub(super) fn restore(value: &mut Value) -> Result<(), serde_yaml_ng::Error> {
        match value {
            Value::Sequence(items) => items.iter_mut().try_for_each(restore),
            Value::Mapping(mapping) => match token_digits(mapping) {
                Some(digits) => {
                    *value = Value::Number(number_from_digits(digits)?);
                    Ok(())
                }
                None => mapping.values_mut().try_for_each(restore),
            },
            Value::Tagged(tagged) => restore(&mut tagged.value),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        }
    }

    /// The digits `mapping` holds, if it is exactly a token mapping.
    fn token_digits(mapping: &Mapping) -> Option<&str> {
        let mut entries = mapping.iter();
        match (entries.next(), entries.next()) {
            (Some((Value::String(key), Value::String(digits))), None) if key == TOKEN => {
                Some(digits)
            }
            _ => None,
        }
    }

    /// The number `serde_json` holds for `digits` when `arbitrary_precision`
    /// is off.
    ///
    /// An integer that fits `u64` is unsigned, a negative one that fits `i64`
    /// is signed, and everything else is a float -- including `-0`, which has
    /// no integer to be. Matching that is what makes YAML under the feature the
    /// YAML the same source text emits without it.
    pub(super) fn number_from_digits(digits: &str) -> Result<Number, serde_yaml_ng::Error> {
        if let Ok(unsigned) = digits.parse::<u64>() {
            return Ok(Number::from(unsigned));
        }
        if let Ok(signed @ i64::MIN..=-1) = digits.parse::<i64>() {
            return Ok(Number::from(signed));
        }
        match digits.parse::<f64>() {
            Ok(float) if float.is_finite() => Ok(Number::from(float)),
            _ => Err(serde_yaml_ng::Error::custom(format_args!(
                "a number beyond the range of a YAML float cannot be emitted: {digits}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
