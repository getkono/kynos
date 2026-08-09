//! What a request-derived input contributes to the description.

use crate::{router::OperationCx, schema::Registry};

/// What a request-derived input contributes to the description.
///
/// This is the trait that makes an undescribable handler fail to compile: there
/// is no blanket implementation, and no way to write one for a type that cannot
/// say what it reads.
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
pub trait RequestContent: Describe {
    /// Every media type accepted by this body extractor.
    fn media_types() -> Vec<&'static str>;

    /// Builds the required OpenAPI Request Body Object for this extractor.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody;
}
