//! The passing half of `request_id_header_that_cannot_carry_one`.
//!
//! The same group, differing in exactly the property under test: it says what
//! carrying one identifier means for the name it declares.

use kynos::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue},
    middleware::request_id::{CorrelationHeaders, RequestId},
    prelude::*,
};

struct TraceId(HeaderValue);

impl HeaderParams for TraceId {
    const NAMES: &'static [&'static str] = &["x-trace-id"];
}

impl EncodeHeaders for TraceId {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        vec![(HeaderName::from_static("x-trace-id"), self.0.clone())]
    }
}

impl CorrelationHeaders for TraceId {
    fn from_id(id: HeaderValue) -> Self {
        Self(id)
    }
}

fn main() {
    let _ = Router::<()>::new().intercept(RequestId::new().header::<TraceId>());
}
