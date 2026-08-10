//! The Path Item Object.

use serde::{Deserialize, Serialize};

use crate::model::{
    extensions::Extensions,
    parameter::Parameter,
    paths::{method::Method, operation::Operation},
    reference::RefOr,
    server::Server,
};

// `Map` backs `additional_operations`, which OpenAPI 3.2 introduced, so the
// import is gated the same way the field is.
#[cfg(feature = "openapi32")]
use crate::Map;

/// The operations available on a single path.
///
/// The per-method slots are boxed. An [`Operation`] is over a kilobyte, and
/// inlining nine of them made this type 8.7 KB — a cost every
/// [`Paths`](crate::model::paths::Paths) entry paid on insert and on rehash.
/// Use [`operation`](PathItem::operation) and
/// [`set_operation`](PathItem::set_operation) rather than touching the fields,
/// and the indirection stays invisible.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathItem {
    /// A reference to an external Path Item.
    ///
    /// The specification leaves the behaviour of fields adjacent to this
    /// undefined, so Kynos never emits one.
    #[serde(rename = "$ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// A summary applying to every operation on this path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A description applying to every operation on this path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The `GET` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get: Option<Box<Operation>>,

    /// The `PUT` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub put: Option<Box<Operation>>,

    /// The `POST` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<Box<Operation>>,

    /// The `DELETE` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<Box<Operation>>,

    /// The `OPTIONS` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Box<Operation>>,

    /// The `HEAD` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Box<Operation>>,

    /// The `PATCH` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<Box<Operation>>,

    /// The `TRACE` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<Box<Operation>>,

    /// The `QUERY` operation.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<Box<Operation>>,

    /// Operations for methods with no dedicated field.
    ///
    /// Introduced in OpenAPI 3.2. Keys are HTTP methods with the exact
    /// capitalization sent on the wire, and must not duplicate a method that
    /// has its own field.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "additionalOperations",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub additional_operations: Map<Box<Operation>>,

    /// Servers serving this path, overriding the document-level list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,

    /// Parameters applying to every operation on this path.
    ///
    /// Hoisting shared parameters here rather than repeating them on each
    /// operation is what keeps a large description readable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RefOr<Parameter>>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl PathItem {
    /// Creates a path item with no operations.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the operation for a method, if declared.
    #[must_use]
    pub fn operation(&self, method: Method) -> Option<&Operation> {
        match method {
            Method::Get => self.get.as_deref(),
            Method::Put => self.put.as_deref(),
            Method::Post => self.post.as_deref(),
            Method::Delete => self.delete.as_deref(),
            Method::Options => self.options.as_deref(),
            Method::Head => self.head.as_deref(),
            Method::Patch => self.patch.as_deref(),
            Method::Trace => self.trace.as_deref(),
            #[cfg(feature = "openapi32")]
            Method::Query => self.query.as_deref(),
        }
    }

    /// Sets the operation for a method, returning any operation it replaced.
    pub fn set_operation(&mut self, method: Method, operation: Operation) -> Option<Operation> {
        let slot = match method {
            Method::Get => &mut self.get,
            Method::Put => &mut self.put,
            Method::Post => &mut self.post,
            Method::Delete => &mut self.delete,
            Method::Options => &mut self.options,
            Method::Head => &mut self.head,
            Method::Patch => &mut self.patch,
            Method::Trace => &mut self.trace,
            #[cfg(feature = "openapi32")]
            Method::Query => &mut self.query,
        };
        slot.replace(Box::new(operation)).map(|boxed| *boxed)
    }

    /// Iterates over the declared operations and their methods.
    pub fn operations(&self) -> impl Iterator<Item = (Method, &Operation)> {
        Method::all()
            .iter()
            .filter_map(move |&method| self.operation(method).map(|op| (method, op)))
    }

    /// Sets the operation for a method, in builder style.
    #[must_use]
    pub fn with_operation(mut self, method: Method, operation: Operation) -> Self {
        self.set_operation(method, operation);
        self
    }
}
