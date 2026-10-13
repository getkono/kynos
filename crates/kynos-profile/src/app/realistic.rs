//! The realistic group's table: `/json/small` behind the stack a production
//! API typically mounts, the composite fixes are prioritised against.

use std::time::Duration;

use kynos::{
    middleware::{
        compression::Compression,
        cors::Cors,
        limits::{body_size::BodySize, timeout::Timeout},
        request_id::RequestId,
        security_headers::SecurityHeaders,
        trace::Trace,
    },
    prelude::*,
    security::schemes::Bearer,
};

use super::{Small, built, extract::Caller, rest, small};
use crate::driver::Served;

/// `json-small`'s handler, behind a bearer guard.
#[kynos::get("/json/small", operation_id = "json_small")]
async fn json_small_guarded(caller: Auth<Bearer<Caller>>) -> Json<Small> {
    let _ = caller.into_inner();
    Json(small())
}

/// Request id, trace, CORS, security headers, a body bound, a deadline and
/// compression around the table, with `/json/small` behind a bearer guard.
///
/// In the order an application would write them: the identifier first, so
/// every other layer's refusal carries it; the trace correlating by it.
pub(crate) fn stack() -> Box<dyn Served> {
    let request_id = RequestId::new();
    let trace = Trace::new().correlating(&request_id);
    built(
        rest()
            .mount(kynos::routes![json_small_guarded])
            .intercept(request_id)
            .observe(trace)
            .intercept(Cors::new().allow_origins(["https://app.example.com"]))
            .intercept(SecurityHeaders::new())
            .intercept(BodySize::new(1_048_576))
            .intercept(Timeout::new(Duration::from_secs(30)))
            .intercept(Compression::new()),
    )
}
