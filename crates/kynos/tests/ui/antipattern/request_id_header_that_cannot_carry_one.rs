//! A `RequestId` header group that cannot be built from an identifier.
//!
//! `HeaderParams::decode` is documented as optional for a group an interceptor
//! only *adds*, which is exactly what `Adds` is. So the group below is written
//! to the trait's own rule — and before `CorrelationHeaders` existed,
//! `RequestId` reached for `decode` anyway and failed once per request instead
//! of once here.

use kynos::{
    extract::params::header::HeaderParams,
    http::{HeaderName, HeaderValue},
    middleware::request_id::RequestId,
    prelude::*,
};

struct TraceId(HeaderValue);

impl HeaderParams for TraceId {
    const NAMES: &'static [&'static str] = &["x-trace-id"];

    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        vec![(HeaderName::from_static("x-trace-id"), self.0.clone())]
    }
}

fn main() {
    let _ = Router::<()>::new().intercept(RequestId::new().header::<TraceId>());
}
