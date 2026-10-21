//! Producing an artifact from a [`Document`], at a chosen specification
//! version.
//!
//! [`crate::model`] is version-agnostic data; turning it into bytes at a
//! particular version lives here.

pub mod downgrade;
#[cfg(feature = "yaml")]
pub mod yaml;

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
        // A version expressing every 3.2 construct has nothing to look for.
        if !version.supports_3_2() {
            let blockers = downgrade::three_two_only_constructs(self);
            if !blockers.is_empty() {
                return Err(SpecError::RequiresV3_2 { blockers });
            }
        }

        let mut emitted = self.clone();
        version.as_str().clone_into(&mut emitted.openapi);
        Ok(emitted)
    }
}

#[cfg(test)]
mod tests;
