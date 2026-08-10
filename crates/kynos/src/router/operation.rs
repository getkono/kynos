//! Describing one operation while the router is built.

use kynos_openapi::{ComponentName, Method, StatusPattern};

use crate::{middleware::contribution::ContributionConflict, schema::registry::Registry};

/// The operation a request matched.
///
/// Handed to an interceptor while the router is built, and to an interceptor or
/// observer while a request is served, so that a metric label, a log field or a
/// rate-limit bucket can be keyed by the operation rather than by the raw path.
/// That is what keeps label cardinality bounded — and because
/// [`path`](Route::path) is the same string that appears as the `paths` key,
/// the label cannot disagree with the description.
///
/// Borrowed and [`Copy`]: nothing here allocates on the request path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Route<'a> {
    path: &'a str,
    operation_id: &'a str,
    method: Method,
}

impl<'a> Route<'a> {
    /// Names an operation.
    // Called by `Router::build`, whose body is still `todo!()`.
    #[allow(dead_code)]
    pub(crate) fn new(path: &'a str, operation_id: &'a str, method: Method) -> Self {
        Self {
            path,
            operation_id,
            method,
        }
    }

    /// The `paths` key this request matched, exactly as the description spells
    /// it — with its `{}` expressions intact, never the request's own path.
    #[must_use]
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// The operation identifier.
    #[must_use]
    pub fn operation_id(&self) -> &'a str {
        self.operation_id
    }

    /// The method.
    #[must_use]
    pub fn method(&self) -> Method {
        self.method
    }
}

/// The description of the operation currently being built.
///
/// Passed to [`Describe`](crate::extract::describe::Describe) implementations
/// so that each handler input can add its own parameters or request body, and
/// to [`Handler::describe`](crate::handler::Handler::describe), which assembles
/// the whole operation from them.
#[derive(Debug)]
pub struct OperationCx<'a> {
    registry: &'a mut Registry,
    operation: kynos_openapi::Operation,
}

impl<'a> OperationCx<'a> {
    /// Begins describing an operation against `registry`.
    pub fn new(registry: &'a mut Registry) -> Self {
        Self {
            registry,
            operation: kynos_openapi::Operation::default(),
        }
    }

    /// Finishes the operation being described.
    #[must_use]
    pub fn finish(self) -> kynos_openapi::Operation {
        self.operation
    }
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

    /// Registers a security scheme under `components`.
    ///
    /// Idempotent for the same scheme under the same name. Two different
    /// schemes under one name are recorded and reported when the router is
    /// built, because a [`Describe`](crate::extract::describe::Describe)
    /// implementation has no way to return an error.
    ///
    /// Without this an `Auth<S>` argument could require a credential it had no
    /// way to declare, and every operation using one would emit a security
    /// requirement naming a scheme the document never defines.
    pub fn add_security_scheme(
        &mut self,
        name: ComponentName,
        scheme: kynos_openapi::SecurityScheme,
    ) {
        let _ = (name, scheme);
        todo!()
    }

    /// Merges responses an input's rejection can produce.
    pub fn add_responses(&mut self, responses: kynos_openapi::Responses) {
        let _ = responses;
        todo!()
    }

    /// Declares a header this input causes the operation to send.
    ///
    /// `WWW-Authenticate` on a 401 is the motivating case: the challenge is
    /// part of what a client must handle, and only the scheme knows it.
    pub fn add_response_header(
        &mut self,
        status: StatusPattern,
        name: impl Into<String>,
        header: kynos_openapi::Header,
    ) {
        let _ = (status, name.into(), header);
        todo!()
    }

    /// Sets the operation identifier.
    pub fn set_operation_id(&mut self, id: &str) {
        let _ = id;
        todo!()
    }

    /// Sets the summary.
    pub fn set_summary(&mut self, summary: &str) {
        let _ = summary;
        todo!()
    }

    /// Sets the description.
    pub fn set_description(&mut self, description: &str) {
        let _ = description;
        todo!()
    }

    /// Marks the operation deprecated.
    pub fn set_deprecated(&mut self, deprecated: bool) {
        let _ = deprecated;
        todo!()
    }

    /// Adds a tag.
    pub fn add_tag(&mut self, name: &str) {
        let _ = name;
        todo!()
    }

    /// Merges an interceptor's declared contribution.
    ///
    /// # Errors
    ///
    /// Returns [`ContributionConflict`] when this contribution and something
    /// already merged disagree about the same part of the description.
    pub fn contribute(
        &mut self,
        contribution: &crate::middleware::contribution::OperationContribution,
    ) -> Result<(), ContributionConflict> {
        let _ = contribution;
        todo!()
    }

    /// The registry, for describing a schema this input needs.
    pub fn registry(&mut self) -> &mut Registry {
        self.registry
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
