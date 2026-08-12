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
//!   appear in `Accept::respond_with`'s bound, and a bound nobody can write down is
//!   a bound nobody can satisfy deliberately — but a private supertrait is what
//!   stops an outside implementation, rather than the module being shut.
//!
//! * **Only the chosen representation is built.** `respond_with` takes closures
//!   rather than values, so the PDF below is rendered for a client that asked
//!   for a PDF and for nobody else. Handing `respond` three finished values
//!   would mean rendering all three and discarding two — work no request asked
//!   for, and invisible until one of the alternatives is expensive.
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
#[derive(Clone, Schema, Serialize, Deserialize)]
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

/// Renders a report as a PDF.
///
/// Stands in for something genuinely expensive — a layout engine, a font
/// cache, a subprocess. It exists so the example is honest about what eager
/// negotiation would have cost: this would run on every request, including the
/// ones that asked for JSON.
fn render_pdf(report: &Report) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n% a real renderer would go here\n".to_vec();
    pdf.extend_from_slice(format!("% {} {}\n", report.month, report.total_cents).as_bytes());
    pdf
}

/// Serves a report as JSON, plain text or a PDF.
///
/// Three arms, one `content` map, and no branch in this function chooses a
/// media type: `respond_with` scores the client's ranked preferences against
/// the tuple's media types and returns the 406 when nothing matches.
///
/// The closures all borrow `report` rather than one of them owning it, which is
/// why the source is passed separately: three arms cannot each take the same
/// value, and a captured one would put that problem in every handler.
#[kynos::get("/reports/{month}")]
async fn get_report(
    Path(path): Path<ReportPath>,
    accept: Accept<ReportFormats>,
) -> Result<Negotiated<ReportFormats>, NegotiationRejection> {
    let report = Report {
        month: path.month,
        total_cents: 1_234_500,
    };

    accept.respond_with(
        &report,
        (
            |report: &Report| Json(report.clone()),
            |report: &Report| Text(format!("{}: {}", report.month, report.total_cents)),
            // Not called unless a PDF is what won.
            |report: &Report| Binary::new(render_pdf(report)),
        ),
    )
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
