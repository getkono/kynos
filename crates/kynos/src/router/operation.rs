//! Describing one operation while the router is built.

use crate::schema::Registry;

/// The description of the operation currently being built.
///
/// Passed to [`Describe`](crate::extract::describe::Describe) implementations
/// so that each handler input can add its own parameters or request body.
#[derive(Debug)]
pub struct OperationCx<'a> {
    _private: std::marker::PhantomData<&'a ()>,
}

impl OperationCx<'_> {
    /// Adds a parameter to the operation.
    pub fn add_parameter(&mut self, parameter: kynos_openapi::Parameter) {
        let _ = parameter;
        todo!()
    }

    /// Sets the operation's request body.
    ///
    /// # Panics
    ///
    /// Panics if a request body was already set. The trait bounds make this
    /// unreachable from a handler — only one argument may implement
    /// [`FromRequest`](crate::extract::FromRequest) — so reaching it indicates
    /// a hand-written [`Describe`](crate::extract::describe::Describe)
    /// implementation that claims a body it does not consume.
    pub fn set_request_body(&mut self, body: kynos_openapi::RequestBody) {
        let _ = body;
        todo!()
    }

    /// Adds a security requirement.
    pub fn add_security(&mut self, requirement: kynos_openapi::SecurityRequirement) {
        let _ = requirement;
        todo!()
    }

    /// Merges responses an input's rejection can produce.
    pub fn add_responses(&mut self, responses: kynos_openapi::Responses) {
        let _ = responses;
        todo!()
    }

    /// The registry, for describing a schema this input needs.
    pub fn registry(&mut self) -> &mut Registry {
        todo!()
    }
}

/// A tag, as a type.
///
/// Derived with `#[derive(Tag)]` on a unit struct. Making tags types rather
/// than strings means a typo is a compile error, and tag-name uniqueness is a
/// property of the module system rather than something checked afterwards.
pub trait Tag {
    /// The tag name as it appears in the description.
    const NAME: &'static str;

    /// The tag's metadata.
    fn metadata() -> kynos_openapi::Tag;
}
