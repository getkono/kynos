//! The `x-kynos-*` annotations: what a waiver leaves on a description.
//!
//! Kynos only lets an application build an API it can describe. Where an
//! escape hatch is taken anyway, the description does not quietly lose the
//! affected part of the service — it records that the part exists and that
//! Kynos did not verify it. These are the field names and shapes that record
//! carries, so that a producer and a checker agree on it by construction
//! rather than by convention.
//!
//! Two records, because there are two situations:
//!
//! | Situation | Record | Why |
//! | --- | --- | --- |
//! | A real operation on a real path, wrapped in something undeclared | [`Opaque`] on the operation | The path is true; only the behaviour is unverified |
//! | A route no path template can express | [`OpaqueRoute`] on the document | Every `paths` key that could be minted would be a lie |
//!
//! [`NOT_AUTHORITATIVE_ANNOTATION`] summarizes both. It is derived — see
//! [`restamp_authority`] — never authored.

use serde::{Deserialize, Serialize};

use crate::model::{document::Document, paths::operation::Operation};

/// The annotation marking a schema as deliberately unconstrained.
///
/// Kynos attaches this wherever a handler used the explicit permissive type, so
/// that "this payload is unchecked" is visible in the published description
/// rather than only in the Rust source.
pub const UNCHECKED_SCHEMA_ANNOTATION: &str = "x-kynos-unchecked";

/// The annotation marking one operation as emitted but unverified.
///
/// Carries an [`Opaque`]. The operation stays in `paths`: an omission is
/// invisible to the consumer that trusts the description, which is strictly
/// worse than a flag it can act on.
pub const OPAQUE_OPERATION_ANNOTATION: &str = "x-kynos-opaque";

/// The annotation listing routes no path template can express.
///
/// Carries an array of [`OpaqueRoute`] at the root of the document.
pub const OPAQUE_ROUTES_ANNOTATION: &str = "x-kynos-opaque-routes";

/// The annotation marking a description as not fully describing the service.
///
/// Derived, never authored: true exactly when some operation carries
/// [`OPAQUE_OPERATION_ANNOTATION`] or some route is recorded under
/// [`OPAQUE_ROUTES_ANNOTATION`]. [`restamp_authority`] computes it.
pub const NOT_AUTHORITATIVE_ANNOTATION: &str = "x-kynos-document-not-authoritative";

/// Why part of a service is not verifiably described.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum OpaqueReason {
    /// A layer of undeclared effect covers the operation.
    ///
    /// It may short-circuit, rewrite the body, or add headers, and its type
    /// says nothing about which.
    UntypedLayer,

    /// The route's matching pattern is not a legal path template.
    ///
    /// A catch-all is the usual case: it matches a set of paths that no single
    /// template describes.
    UntypedRoute,

    /// The route's handler declares neither its inputs nor its responses.
    UntypedHandler,

    /// The route leaves HTTP, so no version of the specification covers it.
    ///
    /// OpenAPI describes request/response semantics. A connection that has
    /// upgraded away from HTTP has no vocabulary here, and inventing one would
    /// produce an entry no consumer could act on.
    ProtocolUpgrade,
}

/// The record a waiver leaves on one operation.
///
/// Serialized under [`OPAQUE_OPERATION_ANNOTATION`]. Marks the operation
/// unverified; never removes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opaque {
    /// Every reason recorded, in the order they were recorded, deduplicated.
    pub reasons: Vec<OpaqueReason>,

    /// Where the waiver was taken, for a human reading the description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Opaque {
    /// A marker carrying one reason.
    #[must_use]
    pub fn new(reason: OpaqueReason) -> Self {
        Self {
            reasons: vec![reason],
            note: None,
        }
    }

    /// Adds a reason, idempotently.
    #[must_use]
    pub fn with_reason(mut self, reason: OpaqueReason) -> Self {
        self.add_reason(reason);
        self
    }

    /// Records where the waiver was taken.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Unions `other`'s reasons into this marker, keeping the first note.
    pub fn absorb(&mut self, other: &Self) {
        for reason in &other.reasons {
            self.add_reason(*reason);
        }
        if self.note.is_none() {
            self.note.clone_from(&other.note);
        }
    }

    fn add_reason(&mut self, reason: OpaqueReason) {
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }

    /// Whether `operation` carries the annotation at all.
    ///
    /// True even when the value is malformed, so that a description Kynos
    /// cannot read is still treated as unverified rather than as clean.
    #[must_use]
    pub fn is_annotated(operation: &Operation) -> bool {
        operation
            .extensions
            .get(OPAQUE_OPERATION_ANNOTATION)
            .is_some()
    }

    /// Reads the marker from an operation.
    ///
    /// `None` when the annotation is absent *or* not in the shape Kynos emits;
    /// [`Opaque::is_annotated`] separates the two, and
    /// [`crate::validate`] reports the malformed case.
    #[must_use]
    pub fn of(operation: &Operation) -> Option<Self> {
        let value = operation.extensions.get(OPAQUE_OPERATION_ANNOTATION)?;
        serde_json::from_value(value.clone()).ok()
    }

    /// Writes the marker onto an operation, merging with any already present.
    ///
    /// # Panics
    ///
    /// Panics only if this marker cannot be serialized, which the type makes
    /// impossible.
    pub fn apply_to(&self, operation: &mut Operation) {
        let mut merged = Self::of(operation).unwrap_or_default();
        merged.absorb(self);
        let value = serde_json::to_value(&merged).expect("an opaque marker is always serializable");
        operation
            .extensions
            .insert(OPAQUE_OPERATION_ANNOTATION, value);
    }
}

/// A route the description cannot express, recorded rather than dropped.
///
/// `pattern` is the router's own matching syntax, verbatim. It is deliberately
/// not a [`PathTemplate`](crate::PathTemplate): minting a template for a
/// catch-all would put a claim in `paths` that the service does not honour —
/// either about the path, or about a parameter whose value always contains an
/// unescaped `/`. A consumer gets something visible, greppable and diffable
/// instead of a plausible lie.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpaqueRoute {
    /// The router's matching pattern, verbatim.
    pub pattern: String,

    /// The literal prefix the pattern is anchored at, if any.
    ///
    /// Recorded so that overlap with a described path can be checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    /// The methods served, spelled as they appear on the wire.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<String>,

    /// Why it cannot be expressed.
    pub reason: OpaqueReason,

    /// A human-readable note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl OpaqueRoute {
    /// Records a route under `pattern` that cannot be described.
    #[must_use]
    pub fn new(pattern: impl Into<String>, reason: OpaqueReason) -> Self {
        Self {
            pattern: pattern.into(),
            prefix: None,
            methods: Vec::new(),
            reason,
            note: None,
        }
    }

    /// Records the literal prefix the pattern is anchored at.
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Records the methods served.
    #[must_use]
    pub fn with_methods<I, S>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.methods = methods.into_iter().map(Into::into).collect();
        self
    }

    /// Records a human-readable note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Whether `document` carries the annotation at all.
    ///
    /// True even when the value is malformed, for the reason
    /// [`Opaque::is_annotated`] gives.
    #[must_use]
    pub fn is_annotated(document: &Document) -> bool {
        document.extensions.get(OPAQUE_ROUTES_ANNOTATION).is_some()
    }

    /// Reads every recorded route from a document.
    ///
    /// `None` when the annotation is present but not in the shape Kynos emits.
    /// An absent annotation reads as an empty list, since no record is the same
    /// claim as an empty record.
    #[must_use]
    pub fn all(document: &Document) -> Option<Vec<Self>> {
        let Some(value) = document.extensions.get(OPAQUE_ROUTES_ANNOTATION) else {
            return Some(Vec::new());
        };
        serde_json::from_value(value.clone()).ok()
    }

    /// Appends this record to a document.
    ///
    /// A malformed existing annotation is replaced rather than extended: it was
    /// not written by Kynos, and silently preserving something unreadable would
    /// leave the list neither one thing nor the other.
    ///
    /// # Panics
    ///
    /// Panics only if this record cannot be serialized, which the type makes
    /// impossible.
    pub fn append_to(&self, document: &mut Document) {
        let mut routes = Self::all(document).unwrap_or_default();
        routes.push(self.clone());
        let value = serde_json::to_value(&routes).expect("an opaque route is always serializable");
        document.extensions.insert(OPAQUE_ROUTES_ANNOTATION, value);
    }
}

/// Every operation on one path item, including any 3.2 additional operation.
fn item_operations(item: &crate::model::paths::item::PathItem) -> impl Iterator<Item = &Operation> {
    let declared = item.operations().map(|(_, operation)| operation);
    #[cfg(feature = "openapi32")]
    let declared = declared.chain(item.additional_operations.values().map(Box::as_ref));
    declared
}

/// Every operation in a document, wherever it is declared.
fn operations(document: &Document) -> impl Iterator<Item = &Operation> {
    document
        .paths
        .0
        .values()
        .chain(document.webhooks.values())
        .chain(document.components.path_items.values())
        .flat_map(item_operations)
}

/// Whether every operation and route in `document` is verifiably described.
///
/// This is the property [`NOT_AUTHORITATIVE_ANNOTATION`] negates. Computing it
/// rather than reading the stamp is deliberate: the stamp is a summary a
/// consumer reads, not the fact itself.
#[must_use]
pub fn is_authoritative(document: &Document) -> bool {
    let no_opaque_routes = match OpaqueRoute::all(document) {
        Some(routes) => routes.is_empty(),
        // Unreadable is not clean.
        None => false,
    };
    no_opaque_routes && !operations(document).any(Opaque::is_annotated)
}

/// Brings [`NOT_AUTHORITATIVE_ANNOTATION`] into line with the document.
///
/// Adds the stamp when something is opaque and removes it when nothing is, so
/// that a document edited after the fact cannot keep a stamp it no longer
/// earns — or lose one it does.
pub fn restamp_authority(document: &mut Document) {
    if is_authoritative(document) {
        document
            .extensions
            .0
            .shift_remove(NOT_AUTHORITATIVE_ANNOTATION);
    } else {
        document
            .extensions
            .insert(NOT_AUTHORITATIVE_ANNOTATION, true);
    }
}

#[cfg(test)]
mod tests;
