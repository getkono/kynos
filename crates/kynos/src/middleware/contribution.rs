//! What an interceptor declares it does to the description.

use kynos_openapi::{ComponentName, ParameterIn, StatusPattern};

/// What an interceptor adds to the description of every operation it covers.
///
/// A closed set, deliberately. If an interceptor does something not expressible
/// here, it is doing something OpenAPI cannot describe, and Kynos would rather
/// not have it.
///
/// Inert data: a contribution can be inspected at build time without running
/// the interceptor that declared it. A description you can only obtain by
/// running the server is a description you cannot check in CI.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct OperationContribution {
    /// Security requirements this interceptor enforces.
    pub security: Vec<kynos_openapi::SecurityRequirement>,

    /// Security schemes to register in `components`.
    pub security_schemes: Vec<(ComponentName, kynos_openapi::SecurityScheme)>,

    /// Parameters this interceptor reads.
    pub parameters: Vec<kynos_openapi::Parameter>,

    /// Responses this interceptor can produce on its own.
    pub responses: kynos_openapi::Responses,

    /// Headers this interceptor adds, and the statuses they appear on.
    ///
    /// A header sent with a 401 and a header sent with a 200 are different
    /// claims, so the status is part of the declaration rather than something
    /// a reader has to infer.
    pub response_headers: Vec<(StatusPattern, String, kynos_openapi::Header)>,

    /// Whether this interceptor marks covered operations deprecated.
    pub deprecated: bool,
}

impl OperationContribution {
    /// An empty contribution.
    ///
    /// What an interceptor returns when it changes nothing a consumer could
    /// observe — response compression, for instance.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether this contribution declares anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.security.is_empty()
            && self.security_schemes.is_empty()
            && self.parameters.is_empty()
            && self.responses.is_empty()
            && self.response_headers.is_empty()
            && !self.deprecated
    }

    /// Declares a response this interceptor can produce.
    #[must_use]
    pub fn with_response(mut self, status: u16, response: kynos_openapi::Response) -> Self {
        let _ = &mut self;
        let _ = (status, response);
        todo!()
    }

    /// Declares the response this interceptor produces for statuses it does not
    /// name individually.
    #[must_use]
    pub fn with_default_response(mut self, response: kynos_openapi::Response) -> Self {
        let _ = &mut self;
        let _ = response;
        todo!()
    }

    /// Declares a response header this interceptor adds, and where it appears.
    #[must_use]
    pub fn with_response_header(
        mut self,
        status: StatusPattern,
        name: impl Into<String>,
        header: kynos_openapi::Header,
    ) -> Self {
        let _ = &mut self;
        let _ = (status, name.into(), header);
        todo!()
    }

    /// Declares a parameter this interceptor reads.
    #[must_use]
    pub fn with_parameter(mut self, parameter: kynos_openapi::Parameter) -> Self {
        let _ = &mut self;
        let _ = parameter;
        todo!()
    }

    /// Declares a security requirement this interceptor enforces.
    #[must_use]
    pub fn with_security(mut self, requirement: kynos_openapi::SecurityRequirement) -> Self {
        let _ = &mut self;
        let _ = requirement;
        todo!()
    }

    /// Registers a security scheme this interceptor's requirements refer to.
    #[must_use]
    pub fn with_security_scheme(
        mut self,
        name: ComponentName,
        scheme: kynos_openapi::SecurityScheme,
    ) -> Self {
        let _ = &mut self;
        let _ = (name, scheme);
        todo!()
    }

    /// Marks every covered operation deprecated.
    #[must_use]
    pub fn with_deprecated(mut self) -> Self {
        let _ = &mut self;
        todo!()
    }

    /// Merges `other` into this contribution, `other` applying after `self`.
    ///
    /// Composition is order-sensitive and does not commute: compression
    /// rewriting headers after authentication has added a 401 produces a
    /// different description than the reverse, and merging must reflect that
    /// rather than sorting it away.
    ///
    /// Some things are not conflicts, and are stated here so an implementation
    /// cannot drift from the contract: `deprecated` is a disjunction, security
    /// requirements append because alternatives are legal, and two declarations
    /// of *the same* value under one key agree rather than disagreeing.
    ///
    /// # Errors
    ///
    /// Returns [`ContributionConflict`] when the two disagree about the same
    /// part of the description — which is how two interceptors that mean
    /// different things by a 429 are caught when the router is built rather
    /// than in production.
    ///
    /// Consuming rather than `&mut self`: a conflict aborts the build, so a
    /// half-merged value should be unrepresentable rather than merely
    /// documented as unspecified. It also makes the router's fold exactly
    /// `try_fold(base, merge)`, which preserves order for free.
    pub fn merge(self, other: Self) -> Result<Self, ContributionConflict> {
        let _ = (&self, other);
        todo!()
    }
}

/// Two interceptors disagreed about the same part of the description.
///
/// One variant per part, rather than a free-text field, so that a caller can
/// tell a contested status from a contested parameter without parsing a
/// message.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ContributionConflict {
    /// Both declared a different response for the same status.
    #[error("two interceptors declare different responses for `{status}`")]
    Response {
        /// The contested status pattern.
        status: StatusPattern,
    },

    /// Both declared a different `default` response.
    #[error("two interceptors declare different `default` responses")]
    DefaultResponse,

    /// Both declared a different header under the same name and status.
    #[error("two interceptors declare different `{name}` headers on `{status}`")]
    ResponseHeader {
        /// The contested header name.
        name: String,
        /// The status it appears on.
        status: StatusPattern,
    },

    /// Both declared a different parameter with the same name and location.
    #[error("two interceptors declare different `in: {location}` parameters named `{name}`")]
    Parameter {
        /// The contested parameter name.
        name: String,
        /// Where it is carried.
        location: ParameterIn,
    },

    /// Both registered a different scheme under one component name.
    #[error("two interceptors register different security schemes named `{name}`")]
    SecurityScheme {
        /// The contested component name.
        name: ComponentName,
    },
}
