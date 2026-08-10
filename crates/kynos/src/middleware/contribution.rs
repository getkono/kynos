//! What an interceptor declares it does to the description.

/// What an interceptor adds to the description of every operation it covers.
///
/// A closed set, deliberately. If an interceptor does something not expressible
/// here, it is doing something OpenAPI cannot describe, and Kynos would rather
/// not have it.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct OperationContribution {
    /// Security requirements this interceptor enforces.
    pub security: Vec<kynos_openapi::SecurityRequirement>,

    /// Security schemes to register in `components`.
    pub security_schemes: Vec<(String, kynos_openapi::SecurityScheme)>,

    /// Parameters this interceptor reads.
    pub parameters: Vec<kynos_openapi::Parameter>,

    /// Responses this interceptor can produce on its own.
    pub responses: kynos_openapi::Responses,

    /// Headers this interceptor adds to responses.
    pub response_headers: Vec<(String, kynos_openapi::Header)>,

    /// Whether this interceptor marks covered operations deprecated.
    pub deprecated: bool,
}

impl OperationContribution {
    /// An empty contribution.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Declares a response this interceptor can produce.
    #[must_use]
    pub fn with_response(mut self, status: u16, response: kynos_openapi::Response) -> Self {
        let _ = &mut self;
        let _ = (status, response);
        todo!()
    }

    /// Declares a response header this interceptor adds.
    #[must_use]
    pub fn with_response_header(
        mut self,
        name: impl Into<String>,
        header: kynos_openapi::Header,
    ) -> Self {
        let _ = &mut self;
        let _ = (name, header);
        todo!()
    }

    /// Merges another contribution into this one.
    ///
    /// # Errors
    ///
    /// Returns [`ContributionConflict`] when both declare a different response
    /// for the same status, which is how two interceptors that disagree about
    /// what a 429 means are caught at build time rather than in production.
    pub fn merge(&mut self, other: Self) -> Result<(), ContributionConflict> {
        let _ = other;
        todo!()
    }
}

/// Two interceptors disagreed about the same part of the description.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("two interceptors declare different descriptions for `{field}`")]
pub struct ContributionConflict {
    /// What they disagreed about.
    pub field: String,
}
