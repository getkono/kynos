//! Choosing a response representation from the client's `Accept` header.
//!
//! ```text
//! cargo run -p kynos --example negotiation
//! ```
//!
//! Three things are worth noticing:
//!
//! * **`Accept` is never a parameter.** The specification says a parameter
//!   definition for that field shall be ignored, so declaring one would put a
//!   claim in the description that no consumer will honour — and
//!   `#[derive(HeaderParams)]` refuses the name for exactly that reason. What
//!   describes the negotiation is the operation's `content` map, which the
//!   representation tuple contributes. `Accept<T>`'s own `Describe` adds only
//!   the rejections, the 406 among them.
//! * **The offer is a type, so the description cannot miss one.** `Negotiated<T>`
//!   carries the same tuple `Accept<T>` was parameterised by, so a
//!   representation the handler can return is a representation the document
//!   lists. There is no way to add an arm at run time.
//! * **`Representation` is sealed, and still nameable.** The offerable set is
//!   exactly the codecs Kynos can describe. Both traits are public — they
//!   appear in `Accept::respond`'s bound, and a bound nobody can write down is
//!   a bound nobody can satisfy deliberately — but a private supertrait is what
//!   stops an outside implementation, rather than the module being shut.
//!
//! Tuple order is meaningful: it breaks the tie when a client's `Accept` ranks
//! two alternatives equally. Put the representation you would rather serve
//! first.

use std::net::Ipv4Addr;

use kynos::{
    error::rejection::NegotiationRejection,
    extract::{
        body::{binary::Binary, text::Text},
        media::Pdf,
    },
    prelude::*,
    response::negotiate::{
        Accept, Negotiated,
        representation::{Representation, Representations},
    },
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A monthly report.
#[derive(Schema, Serialize, Deserialize)]
struct Report {
    month: String,
    total_cents: u64,
}

/// What `/reports/{month}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct ReportPath {
    month: String,
}

/// The three ways this service will serve a report.
///
/// A type alias rather than three spellings, so the extractor, the return type
/// and the offer cannot drift apart — they are the same tuple by construction.
/// JSON first: it is what an integration wants, and it wins a tie.
type ReportFormats = (Json<Report>, Text, Binary<Pdf>);

/// Serves a report as JSON, plain text or a PDF.
///
/// Three arms, one `content` map, and no branch in this function chooses a
/// media type: `respond` scores the client's ranked preferences against the
/// tuple's media types and returns the 406 when nothing matches.
#[kynos::get("/reports/{month}")]
async fn get_report(
    Path(path): Path<ReportPath>,
    accept: Accept<ReportFormats>,
) -> Result<Negotiated<ReportFormats>, NegotiationRejection> {
    let report = Report {
        month: path.month,
        total_cents: 1_234_500,
    };
    let text = format!("{}: {}", report.month, report.total_cents);

    accept.respond((Json(report), Text(text), Binary::new(Vec::<u8>::new())))
}

/// A program generic over what it offers.
///
/// This is why the traits are public rather than merely sealed. The bound is
/// writable, so a helper can be generic over an offer — and it is still closed,
/// so the offer can only be codecs the description knows.
fn media_types_offered<T: Representations>() -> Vec<&'static str> {
    T::media_types()
}

/// The same, for one alternative rather than a tuple.
fn media_type_of<T: Representation>() -> &'static str {
    T::media_type()
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    // The offer is knowable without a request, which is what makes it
    // describable at build time.
    println!("offering {:?}", media_types_offered::<ReportFormats>());
    println!("the first is {}", media_type_of::<Json<Report>>());

    let router = Router::<()>::new().mount(kynos::routes![get_report]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
