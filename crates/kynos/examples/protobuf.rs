//! Protocol Buffers as a request and response body.
//!
//! Run it with the protobuf codec on and JSON off, since nothing here is JSON:
//!
//! ```text
//! cargo run -p kynos --example protobuf --no-default-features \
//!   --features openapi31,macros,server,http1,protobuf
//! ```
//!
//! Two things are worth noticing:
//!
//! * **A message derives twice, and the two derives answer different
//!   questions.** `prost::Message` decides the bytes; `kynos::Schema` decides
//!   what the description says the bytes mean. Kynos does not generate one from
//!   the other, and the reason is that it cannot: a `.proto` field number has no
//!   JSON Schema meaning, and a JSON Schema constraint has no protobuf
//!   encoding. Writing both is the honest cost of describing a binary codec in
//!   a JSON-Schema vocabulary.
//! * **The schema describes the message, not the wire bytes.** A protobuf body
//!   is not self-describing, so what goes in the operation's `content` map
//!   under `application/protobuf` is the *shape* — which is what a consumer
//!   generating a client from the description needs, and all a JSON Schema can
//!   say about a binary encoding.
//!
//! The field numbers are what make a protobuf message forward-compatible, and
//! they are visible only in the `prost` attributes. That asymmetry is worth
//! seeing: a reader who changes `tag = "2"` has changed the wire format without
//! changing the description, and nothing here catches it — protobuf's own
//! compatibility rules are the thing that does.

use std::net::Ipv4Addr;

use kynos::{extract::body::protobuf::Protobuf, prelude::*, server::Server};

/// A telemetry sample, as it travels.
///
/// `Message` and `Schema` are both derived: one is the encoding, the other is
/// the contract. `Default` is required by the extractor, because decoding a
/// protobuf message starts from one and fills in the fields that arrived.
#[derive(Clone, PartialEq, prost::Message, Schema)]
struct Sample {
    /// The series this sample belongs to.
    #[prost(string, tag = "1")]
    #[schema(min_length = 1, max_length = 128)]
    series: String,

    /// Milliseconds since the Unix epoch.
    ///
    /// A protobuf `int64` and an OpenAPI `int64`, which happen to agree. They
    /// need not: `uint32` is a registered OpenAPI format and a protobuf type,
    /// and the derives would each pick their own.
    #[prost(int64, tag = "2")]
    at_millis: i64,

    /// The measured value.
    #[prost(double, tag = "3")]
    value: f64,
}

/// A batch of samples.
#[derive(Clone, PartialEq, prost::Message, Schema)]
struct Batch {
    /// Repeated in protobuf is a `Vec` in Rust and an `array` in the schema —
    /// three spellings of one fact, which is the most agreement the two
    /// vocabularies reach.
    #[prost(message, repeated, tag = "1")]
    #[schema(max_items = 1_000)]
    samples: Vec<Sample>,
}

/// How many samples were stored.
#[derive(Clone, PartialEq, prost::Message, Schema)]
struct Receipt {
    #[prost(uint32, tag = "1")]
    stored: u32,
}

/// Accepts a batch of samples.
///
/// Protobuf in, protobuf out. The same wrapper describes both directions, so
/// the request body and the 200 response are keyed by the same media type
/// without either being written twice.
#[kynos::post("/samples")]
async fn ingest(Protobuf(batch): Protobuf<Batch>) -> Protobuf<Receipt> {
    let _ = batch;
    todo!("the router is still a skeleton; this example exists to typecheck")
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![ingest]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
