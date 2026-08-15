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
    ///
    /// A set constraint replaces the keyword the type itself emitted, which is
    /// what makes a field declaration the narrower statement it reads as. An
    /// unset one leaves that keyword alone, so an empty set — and every field
    /// of one — is a no-op.
    ///
    /// The keywords land beside a `$ref` rather than under an `allOf`: from
    /// OpenAPI 3.1 onward a schema `$ref` applies its siblings, so a
    /// constrained field of a named type is the intersection it looks like.
    #[must_use]
    pub fn apply(&self, schema: OpenApiSchema) -> OpenApiSchema {
        // Nothing to say, and nothing a schema that admits no instance could
        // be narrowed by.
        if self.is_empty() || matches!(schema, OpenApiSchema::Bool(false)) {
            return schema;
        }

        // `true` and the empty keyword set are the same schema, so promoting
        // one to the other loses nothing and gives the keywords somewhere to
        // go.
        let mut object = match schema {
            OpenApiSchema::Object(object) => object,
            OpenApiSchema::Bool(_) => Box::default(),
        };

        macro_rules! set {
            ($($field:ident),+ $(,)?) => {
                $(
                    if self.$field.is_some() {
                        object.$field.clone_from(&self.$field);
                    }
                )+
            };
        }

        set!(
            minimum,
            maximum,
            exclusive_minimum,
            exclusive_maximum,
            multiple_of,
            min_length,
            max_length,
            pattern,
            min_items,
            max_items,
            unique_items,
            format,
        );

        OpenApiSchema::Object(object)
    }
}
