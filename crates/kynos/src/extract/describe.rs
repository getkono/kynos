//! What a request-derived input contributes to the description.

use crate::{router::operation::OperationCx, schema::registry::Registry};

/// What a request-derived input contributes to the description.
///
/// This is the trait that makes an undescribable handler fail to compile: there
/// is no blanket implementation, and no way to write one for a type that cannot
/// say what it reads.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not describe itself, so it cannot be a handler argument",
    label = "not describable",
    note = "every handler argument declares what it reads: `Path<T>`, `Query<T>`, `Headers<T>`, \
            `Json<T>` and the rest. There is deliberately no extractor for the raw request, \
            because one would let an operation read something its description never mentions"
)]
pub trait Describe {
    /// Adds this input's parameters or request body to the operation.
    fn describe(operation: &mut OperationCx<'_>);
}

/// Metadata shared by request-body extractors.
///
/// This trait deliberately has no implementation for request-part extractors.
/// Consequently `Option<Path<T>>` and similar ambiguous signatures do not
/// compile, while `Option<Json<T>>` means that the entire body is optional.
///
/// ```compile_fail
/// fn body<T: kynos::extract::FromRequest<()>>() {}
/// body::<Option<kynos::extract::params::path::Path<u64>>>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a request body",
    label = "not a request body",
    note = "a body is one of `Json<T>`, `Form<T>`, `MultipartForm<T>`, `Protobuf<T>`, `Text` or \
            `Binary<M>`, optionally wrapped in `Option` or `OneOf`",
    note = "only the last handler argument may consume the body"
)]
pub trait RequestContent: Describe {
    /// Every media type accepted by this body extractor.
    fn media_types() -> Vec<&'static str>;

    /// Builds the required OpenAPI Request Body Object for this extractor.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody;
}
