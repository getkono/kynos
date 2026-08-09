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
    /// Returns an error only if a specification extension holds a value that
    /// cannot be represented in YAML.
    #[cfg(feature = "yaml")]
    pub fn to_yaml(&self) -> Result<String, serde_yaml_ng::Error> {
        serde_yaml_ng::to_string(self)
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

#[cfg(test)]
mod tests;
