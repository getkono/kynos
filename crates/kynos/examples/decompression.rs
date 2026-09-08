//! Accepting a request body the client compressed.
//!
//! ```text
//! cargo run -p kynos --example decompression --features compression
//! ```
//!
//! Then send the same payload twice, once plain and once gzipped, and watch the
//! handler see no difference:
//!
//! ```text
//! curl -s localhost:3000/measurements -H 'content-type: application/json' \
//!   --data '{"readings":[1,2,3]}'
//!
//! printf '{"readings":[1,2,3]}' | gzip | curl -s localhost:3000/measurements \
//!   -H 'content-type: application/json' -H 'content-encoding: gzip' --data-binary @-
//! ```
//!
//! Five things are worth noticing:
//!
//! * **This is not `Accept-Encoding` in reverse.** RFC 9110 section 12.5.3's
//!   `Accept-Encoding` is a client saying what it will *receive*, and it says
//!   nothing about what it may send. What a client sends is announced in
//!   `Content-Encoding` (section 8.4) and is not negotiated at all: it arrives,
//!   and the server either understands it or refuses it with 415. The two
//!   directions are separate decisions, which is why they are separate
//!   interceptors — mounting [`Compression`] does not make a service accept
//!   compressed uploads, and this does not make it send compressed responses.
//! * **`BodySize` is not the guard here, and mounting it is a compile error.**
//!   Two kilobytes of zeroes are a gigabyte of gzip output, so a limit measured
//!   before decoding measures the one number an attacker sets freely. The limit
//!   `Decompression` takes is the route's body limit applied to what the
//!   handler will actually see. Both answer 413, so `statuses_disjoint` refuses
//!   the pair — correctly, since it would be ambiguous as well as redundant.
//! * **The refusals are declared, and appear in the description.** 415, 413 and
//!   400 reach every covered operation because [`Undecodable`] is the
//!   interceptor's `Short` type. A client generator therefore knows to expect
//!   them without anyone remembering to write them down.
//! * **Each refusal names its own RFC 9457 `type`, and there are three.** A
//!   client branches on `type`, and `about:blank` says the status code is the
//!   whole story — true of neither of these. So each is named with a marker of
//!   its own, and the declared response narrows `type` to that URI rather than
//!   merely exemplifying it: a body disagreeing with its own declaration fails
//!   the conformance harness. One marker for all three would say a malformed
//!   body and an unsupported coding are the same problem.
//! * **What is stripped is stripped because it stopped being true.** Section
//!   8.4 says the representation *is* the coded form, and that all other
//!   metadata about it describes that form. Once the coded form is gone,
//!   `Content-Length` names the wrong number and `Content-Digest` names octets
//!   nothing holds — so they go, rather than being left to be checked against a
//!   body they were never computed over.
//!
//! [`Compression`]: kynos::middleware::compression::Compression
//! [`Undecodable`]: kynos::middleware::decompression::Undecodable

use std::net::Ipv4Addr;

use kynos::{
    error::problem::ProblemType, middleware::decompression::Decompression, prelude::*,
    response::status::Accepted, server::Server,
};

/// What this service calls a coding it cannot decode.
///
/// A marker rather than a value: what an interceptor declares is read from its
/// associated types, so a URI chosen per request would reach the wire and leave
/// the description saying `about:blank` about it.
struct UnknownCoding;

impl ProblemType for UnknownCoding {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/unknown-coding");
}

/// What it calls a body that is not the coding it claimed.
struct NotWhatItClaimed;

impl ProblemType for NotWhatItClaimed {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/malformed-body");
}

/// What it calls a payload that expands past the limit.
///
/// The one an operator most wants told apart: a client whose batch is simply
/// too big retries smaller, and a client sending a decompression bomb does not.
struct Bomb;

impl ProblemType for Bomb {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/decompression-bomb");
}

/// A batch of sensor readings.
///
/// The shape this example exists for: a payload big enough and repetitive
/// enough that a client sending thousands of them a minute will compress, and
/// small enough on the wire afterwards that nothing between here and there
/// notices.
#[derive(Debug, Schema, serde::Deserialize, serde::Serialize)]
struct Measurements {
    /// The readings, in the order they were taken.
    readings: Vec<f64>,
}

/// Records a batch of readings.
///
/// Nothing here knows whether the body arrived compressed. That is the whole
/// point: `Json<Measurements>` is handed the decoded octets, so a handler never
/// grows a branch for a transport decision.
#[kynos::post("/measurements")]
async fn record(Json(batch): Json<Measurements>) -> Accepted<Json<Measurements>> {
    Accepted::new(Json(batch))
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        .mount(kynos::routes![record])
        // Four megabytes decoded, whatever arrived. The ratio is left off:
        // the absolute limit already bounds what a request can cost, and a
        // ratio tight enough to be worth having under gzip refuses payloads
        // zstd produces legitimately. Set it once you have measured your own.
        .intercept(
            Decompression::new(4 * 1024 * 1024)
                // Three refusals, three URIs. Each builder is available only
                // while its own refusal is unnamed, so they may be written in
                // any order and none can be named twice.
                .unsupported_coding_problem_type::<UnknownCoding>()
                .malformed_problem_type::<NotWhatItClaimed>()
                .too_large_problem_type::<Bomb>(),
        );

    // 400, 413 and 415 are on every operation below, contributed by the
    // interceptor rather than written out by hand -- which is the property that
    // makes them impossible to forget. Each one's schema narrows `type` to the
    // URI named above, so a consumer reads which problem a status carries
    // rather than only that it carries a problem.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
