//! Field constraints, as one declaration the document and the parser share.

use kynos_openapi::Schema as OpenApiSchema;

/// Constraints attached to a field by `#[derive(Schema)]`.
///
/// These become JSON Schema assertions, which means the emitted description and
/// the request parser are two projections of one declaration. There is no
/// separate validation pass, and no JSON Schema interpreter on the hot path.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Constraints {
    /// `minimum`, for numeric fields.
    pub minimum: Option<f64>,
    /// `maximum`, for numeric fields.
    pub maximum: Option<f64>,
    /// `exclusiveMinimum`, for numeric fields.
    pub exclusive_minimum: Option<f64>,
    /// `exclusiveMaximum`, for numeric fields.
    pub exclusive_maximum: Option<f64>,
    /// `multipleOf`, for numeric fields.
    pub multiple_of: Option<f64>,
    /// `minLength`, for string fields.
    pub min_length: Option<u64>,
    /// `maxLength`, for string fields.
    pub max_length: Option<u64>,
    /// `pattern`, an ECMA-262 regular expression, for string fields.
    pub pattern: Option<String>,
    /// `minItems`, for array fields.
    pub min_items: Option<u64>,
    /// `maxItems`, for array fields.
    pub max_items: Option<u64>,
    /// `uniqueItems`, for array fields.
    pub unique_items: Option<bool>,
    /// `format`, a semantic annotation such as `uuid` or `date-time`.
    pub format: Option<String>,
}

impl Constraints {
    /// Whether any constraint is set.
    ///
    /// An empty set applied to a schema leaves it unchanged, so a caller that
    /// would emit the result as a keyword can skip it entirely.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Applies these constraints to a schema.
    #[must_use]
    pub fn apply(&self, schema: OpenApiSchema) -> OpenApiSchema {
        let _ = schema;
        todo!()
    }
}
