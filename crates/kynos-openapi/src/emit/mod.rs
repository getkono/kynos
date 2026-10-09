//! Producing an artifact from a [`Document`], at a chosen specification
//! version.
//!
//! [`crate::model`] is version-agnostic data; turning it into bytes at a
//! particular version lives here.

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
    /// Returns an error only if a number cannot be written as a YAML number,
    /// which can happen only when `serde_json`'s `arbitrary_precision` feature
    /// is unified into the build: a number beyond the range of a 64-bit float,
    /// or a hand-built object shaped like its private number token.
    #[cfg(feature = "yaml")]
    pub fn to_yaml(&self) -> Result<String, YamlError> {
        // Only token-mapping builds take the detour through a `Value`, which
        // keeps one value per key where direct serialization writes every key.
        if !yaml_numbers::serialized_as_token() {
            return serde_yaml_ng::to_string(self).map_err(YamlError);
        }
        let mut value = serde_yaml_ng::to_value(self).map_err(YamlError)?;
        yaml_numbers::restore(&mut value).map_err(YamlError)?;
        serde_yaml_ng::to_string(&value).map_err(YamlError)
    }

    /// Produces this document as `version`, refusing a lossy downgrade.
    ///
    /// The way to get a 3.1 description from a build with `openapi32` enabled:
    /// rather than dropping 3.2-only constructs, it names what stands in the way.
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

/// The failure [`Document::to_yaml`] returns.
///
/// Opaque, so the pre-1.0 YAML library behind it stays out of this crate's API;
/// its message is this error's [`source`](std::error::Error::source).
///
/// Construct one outside this crate through [`serde::ser::Error`].
#[cfg(feature = "yaml")]
#[derive(Debug, thiserror::Error)]
#[error("the description could not be emitted as YAML")]
pub struct YamlError(#[source] serde_yaml_ng::Error);

#[cfg(feature = "yaml")]
impl serde::ser::Error for YamlError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self(<serde_yaml_ng::Error as serde::ser::Error>::custom(message))
    }
}

/// Numbers as YAML writes them, whatever `serde_json` features the build
/// unifies.
///
/// Under `serde_json/arbitrary_precision` a number serializes as a private-token
/// mapping, which `serde_yaml_ng` writes verbatim; since a crate cannot `cfg` on
/// a dependency's features, this rewrites each such mapping back to a number.
#[cfg(feature = "yaml")]
mod yaml_numbers {
    use serde::ser::Error as _;
    use serde_yaml_ng::{Mapping, Number, Value};

    /// Copy of `serde_json`'s `pub(crate)` `TOKEN` in `number.rs`; must stay in
    /// sync, which `mise run test:arbitrary-precision` checks.
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
    /// is signed, and everything else (including `-0`) is a float.
    pub(super) fn number_from_digits(digits: &str) -> Result<Number, serde_yaml_ng::Error> {
        if let Ok(unsigned) = digits.parse::<u64>() {
            return Ok(Number::from(unsigned));
        }
        if let Ok(signed @ i64::MIN..=-1) = digits.parse::<i64>() {
            return Ok(Number::from(signed));
        }
        match digits.parse::<f64>() {
            Ok(float) if float.is_finite() => Ok(Number::from(float)),
            // Digits beyond any float, or a token-shaped object built by hand
            // whose string is no number: neither has a YAML number to be.
            _ => Err(serde_yaml_ng::Error::custom(format_args!(
                "a number serde_json holds as `{digits}` cannot be emitted as a YAML number"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
