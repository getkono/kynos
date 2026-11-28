//! Producing an artifact from a [`Document`], at a chosen specification
//! version.
//!
//! [`crate::model`] is version-agnostic data; turning it into bytes at a
//! particular version lives here.

mod canonical;
pub mod downgrade;
#[cfg(feature = "yaml")]
pub mod yaml;

use crate::{
    model::document::{Document, SpecVersion},
    validate::violation::SpecError,
};

impl Document {
    /// Serializes to pretty-printed JSON, in the model's order.
    ///
    /// Each object's fields come in the order the model declares them, then
    /// its extensions, and each map — `paths`, `components`, `responses` and
    /// the rest — in insertion order, which for a router's description is the
    /// order `docs/schema.md` records. Both are fixed by the code that built
    /// the document, so the same code emits the same bytes in every run.
    ///
    /// The free-form JSON a document carries — an example, a `default`, a
    /// `const`, an extension's value — is the exception: its objects are in
    /// `serde_json`'s order, sorted unless some crate in the build enables
    /// `serde_json/preserve_order`, and in insertion order if one does. For
    /// bytes no build changes, as a committed description wants, use
    /// [`to_canonical_json`](Self::to_canonical_json).
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
