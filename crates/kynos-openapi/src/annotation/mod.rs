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
//! [`Document::restamp_authority`] — never authored.

use serde::{Deserialize, Serialize};

use crate::model::{
    document::Document,
    paths::{item::PathItem, operation::Operation},
    reference::RefOr,
};

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
/// [`OPAQUE_ROUTES_ANNOTATION`]. [`Document::restamp_authority`] computes it.
pub const NOT_AUTHORITATIVE_ANNOTATION: &str = "x-kynos-document-not-authoritative";

/// Why part of a service is not verifiably described.
///
/// Deliberately not `Copy`: the wire form has to survive a description written
/// by a newer Kynos, which means carrying a reason this build does not know as
/// [`Unrecognized`](OpaqueReason::Unrecognized) rather than failing to read the
/// record at all.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    /// The route serves a tree of files whose membership is not fixed.
    ///
    /// A catch-all like every other, so [`UntypedRoute`](Self::UntypedRoute)
    /// would be true of it — but it reads identically to a business API someone
    /// wildcarded, and the two deserve different amounts of alarm. A consumer
    /// meeting this knows the undescribed part of the service is a directory of
    /// files rather than an operation nobody wrote down, and a CI gate can
    /// tolerate exactly this one.
    StaticAssets,

    /// A reason recorded by a version of Kynos that knows more than this one.
    ///
    /// Preserved verbatim so the record round-trips. An older reader must not
    /// turn a description it merely does not fully understand into one it
    /// reports as malformed -- and must not drop the reason when it writes the
    /// document back out.
    #[serde(untagged)]
    Unrecognized(String),
}

impl OpaqueReason {
    /// The reason as it is spelled in the description.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::UntypedLayer => "untyped-layer",
            Self::UntypedRoute => "untyped-route",
            Self::UntypedHandler => "untyped-handler",
            Self::ProtocolUpgrade => "protocol-upgrade",
            Self::StaticAssets => "static-assets",
            Self::Unrecognized(reason) => reason,
        }
    }
}

impl std::fmt::Display for OpaqueReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The record a waiver leaves on one operation.
///
/// Serialized under [`OPAQUE_OPERATION_ANNOTATION`]. Marks the operation
/// unverified; never removes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
            self.add_reason(reason.clone());
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
    /// `Ok(None)` means the operation carries no marker. A reason this build
    /// does not know is *not* an error — it round-trips as
    /// [`OpaqueReason::Unrecognized`] — so an error here means the value was
    /// hand-written into a shape Kynos never emits.
    ///
    /// # Errors
    ///
    /// Returns [`MalformedAnnotation`] when the annotation is present but
    /// unreadable.
    pub fn of(operation: &Operation) -> Result<Option<Self>, MalformedAnnotation> {
        let Some(value) = operation.extensions.get(OPAQUE_OPERATION_ANNOTATION) else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| MalformedAnnotation::new(OPAQUE_OPERATION_ANNOTATION, &error))
    }

    /// Writes the marker onto an operation, merging with any already present.
    ///
    /// # Errors
    ///
    /// Returns [`MalformedAnnotation`] when the operation already carries an
    /// unreadable marker, rather than replacing it. Overwriting would delete a
    /// waiver someone recorded, which is the one thing this whole mechanism
    /// exists to prevent.
    ///
    /// # Panics
    ///
    /// Panics only if this marker cannot be serialized, which the type makes
    /// impossible.
    pub fn apply_to(&self, operation: &mut Operation) -> Result<(), MalformedAnnotation> {
        let mut merged = Self::of(operation)?.unwrap_or_default();
        merged.absorb(self);
        let value = serde_json::to_value(&merged).expect("an opaque marker is always serializable");
        operation
            .extensions
            .insert(OPAQUE_OPERATION_ANNOTATION, value);
        Ok(())
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
#[non_exhaustive]
pub struct OpaqueRoute {
    /// The router's matching pattern, verbatim.
    pub pattern: String,

    /// The literal prefix the pattern is anchored at, if any.
    ///
    /// Recorded so that a reader can tell which part of the URL space the
    /// route claims without parsing the router's matching syntax.
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
    /// An absent annotation reads as an empty list, since recording nothing is
    /// the same claim as recording an empty list.
    ///
    /// # Errors
    ///
    /// Returns [`MalformedAnnotation`] when the annotation is present but
    /// unreadable. A reason this build does not know is not that case — it
    /// round-trips as [`OpaqueReason::Unrecognized`].
    pub fn all(document: &Document) -> Result<Vec<Self>, MalformedAnnotation> {
        let Some(value) = document.extensions.get(OPAQUE_ROUTES_ANNOTATION) else {
            return Ok(Vec::new());
        };
        serde_json::from_value(value.clone())
            .map_err(|error| MalformedAnnotation::new(OPAQUE_ROUTES_ANNOTATION, &error))
    }

    /// Appends this record to a document.
    ///
    /// # Errors
    ///
    /// Returns [`MalformedAnnotation`] when the document already carries an
    /// unreadable list, rather than replacing it. Appending by overwriting
    /// would delete every route someone else recorded — silent loss of exactly
    /// the record this mechanism exists to keep.
    ///
    /// # Panics
    ///
    /// Panics only if this record cannot be serialized, which the type makes
    /// impossible.
    pub fn append_to(&self, document: &mut Document) -> Result<(), MalformedAnnotation> {
        let mut routes = Self::all(document)?;
        routes.push(self.clone());
        let value = serde_json::to_value(&routes).expect("an opaque route is always serializable");
        document.extensions.insert(OPAQUE_ROUTES_ANNOTATION, value);
        Ok(())
    }
}

/// A Kynos annotation was present but not in the shape Kynos emits.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{name}` is present but is not in the form Kynos emits: {detail}")]
pub struct MalformedAnnotation {
    /// The offending field name.
    pub name: String,
    /// What went wrong reading it.
    pub detail: String,
}

impl MalformedAnnotation {
    fn new(name: &str, error: &serde_json::Error) -> Self {
        Self {
            name: name.to_owned(),
            detail: error.to_string(),
        }
    }
}

/// Every operation reachable from one path item.
///
/// Callbacks are path items in their own right, and an operation inside one is
/// as much part of the service as any other — so a waiver taken there has to be
/// as visible. Boxed because the recursion is not otherwise expressible.
fn item_operations(item: &PathItem) -> Box<dyn Iterator<Item = &Operation> + '_> {
    let declared = item.operations().map(|(_, operation)| operation);
    #[cfg(feature = "openapi32")]
    let declared = declared.chain(item.additional_operations.values().map(Box::as_ref));

    Box::new(declared.flat_map(|operation| {
        std::iter::once(operation).chain(
            operation
                .callbacks
                .values()
                .filter_map(RefOr::as_item)
                .flat_map(|callback| callback.0.values())
                .filter_map(RefOr::as_item)
                .flat_map(item_operations),
        )
    }))
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
        .chain(
            document
                .components
                .callbacks
                .values()
                .filter_map(RefOr::as_item)
                .flat_map(|callback| callback.0.values())
                .filter_map(RefOr::as_item)
                .flat_map(item_operations),
        )
}

impl Document {
    /// Whether every operation and route in this document is verifiably
    /// described.
    ///
    /// This is the property [`NOT_AUTHORITATIVE_ANNOTATION`] negates. Computing
    /// it rather than reading the stamp is deliberate: the stamp is a summary a
    /// consumer reads, not the fact itself. An annotation this build cannot
    /// read counts as unclean, because the alternative is calling a description
    /// authoritative on the strength of not understanding it.
    #[must_use]
    pub fn is_authoritative(&self) -> bool {
        let no_opaque_routes = OpaqueRoute::all(self).is_ok_and(|routes| routes.is_empty());
        no_opaque_routes && !operations(self).any(Opaque::is_annotated)
    }

    /// Brings [`NOT_AUTHORITATIVE_ANNOTATION`] into line with this document.
    ///
    /// Adds the stamp when something is opaque and removes it when nothing is,
    /// so that a document edited after the fact cannot keep a stamp it no
    /// longer earns — or lose one it does.
    pub fn restamp_authority(&mut self) {
        if self.is_authoritative() {
            self.extensions.remove(NOT_AUTHORITATIVE_ANNOTATION);
        } else {
            self.extensions.insert(NOT_AUTHORITATIVE_ANNOTATION, true);
        }
    }
}

#[cfg(test)]
mod tests;
