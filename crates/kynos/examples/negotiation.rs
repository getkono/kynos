//! Choosing a response representation from the client's `Accept` header.
//!
//! ```text
//! cargo run -p kynos --example negotiation
//! ```
//!
//! Worth noticing:
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
//! * **`Representation` is sealed, and still nameable.** Every offer is a body
//!   type whose `content` entry Kynos writes. Both traits are public — they
//!   appear in `Accept::respond_with`'s bound, and a bound nobody can write down is
//!   a bound nobody can satisfy deliberately — but a private supertrait is what
//!   stops an outside implementation, rather than the module being shut.
//!
//! * **An outside format is offered through its codec, not the trait.**
//!   `ReportLines` below is a `BodyCodec` for a line-oriented format Kynos does
//!   not ship. `Encoded<ReportLines, Report>` is read, offered beside `Json` and
//!   described under its own `content` key, while the 415, the body limit, the
//!   400/422 split and the schema stay Kynos's: `PUT /reports/{month}` answers
//!   400 for a body that is not the format, and 422 for one whose total is not
//!   a `u64`.
//!
//! * **Only the chosen representation is built.** `respond_with` takes closures
//!   rather than values, so the PDF below is rendered for a client that asked
//!   for a PDF and for nobody else. Handing `respond` three finished values
//!   would mean rendering all three and discarding two — work no request asked
//!   for, and invisible until one of the alternatives is expensive.
//!
//! * **A version is a media type.** `JsonAs<M, T>` is `Json<T>` under the
//!   `+json` type `M` names. `/reports/{month}/versioned` offers two versions
//!   of the report, each with its own schema, and the client picks one in
//!   `Accept`.
//!
//! Tuple order is meaningful: it breaks the tie when a client's `Accept` ranks
//! two alternatives equally. Put the representation you would rather serve
//! first.

use std::{io::Write, net::Ipv4Addr};

use bytes::Bytes;
use kynos::{
    error::rejection::NegotiationRejection,
    extract::body::{
        binary::Binary,
        codec::{BodyCodec, DecodeError, Encoded},
        json_as::JsonAs,
        text::Text,
    },
    http::media::{MediaType, Pdf},
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

/// Version 1 of the report document: `Report` as it was first published.
struct ReportV1;

impl MediaType for ReportV1 {
    const MEDIA_TYPE: &'static str = "application/vnd.example.report.v1+json";
}

/// Version 2, which states its currency.
struct ReportV2;

impl MediaType for ReportV2 {
    const MEDIA_TYPE: &'static str = "application/vnd.example.report.v2+json";
}

/// A report as version 2 of the document carries it.
#[derive(Schema, Serialize, Deserialize)]
struct PricedReport {
    month: String,
    total_cents: u64,
    currency: String,
}

/// Both versions of the report, newest first so that it wins a tie.
///
/// A client pins a version with
/// `Accept: application/vnd.example.report.v1+json`, and one sending `*/*`
/// gets the newest. The description lists each version under its own
/// `content` key, with its own schema.
type VersionedReport = (JsonAs<ReportV2, PricedReport>, JsonAs<ReportV1, Report>);

/// Serves either version of a report.
#[kynos::get("/reports/{month}/versioned")]
async fn get_versioned_report(
    Path(path): Path<ReportPath>,
    accept: Accept<VersionedReport>,
) -> Result<Negotiated<VersionedReport>, NegotiationRejection> {
    let report = Report {
        month: path.month,
        total_cents: 1_234_500,
    };

    accept.respond_with(
        &report,
        (
            |report: &Report| {
                JsonAs::new(PricedReport {
                    month: report.month.clone(),
                    total_cents: report.total_cents,
                    currency: "EUR".to_owned(),
                })
            },
            |report: &Report| JsonAs::new(report.clone()),
        ),
    )
}

/// A report as `month: …` and `total_cents: …` lines, in that order: a format
/// Kynos does not ship, brought by the application.
///
/// The codec asserts that `Report`'s schema describes this format, which it
/// does: one string and one non-negative integer, under the same names.
struct ReportLines;

impl BodyCodec<Report> for ReportLines {
    const MEDIA_TYPE: &'static str = "application/vnd.example.report-lines";

    fn decode(bytes: &Bytes) -> Result<Report, DecodeError> {
        let malformed = || DecodeError::Syntax("expected `month:` and `total_cents:` lines".into());

        let text = std::str::from_utf8(bytes).map_err(|_| malformed())?;
        let mut lines = text.lines();
        let mut field = |name: &str| {
            lines
                .next()
                .and_then(|line| line.strip_prefix(name)?.strip_prefix(':'))
                .map(str::trim)
                .ok_or_else(malformed)
        };
        let month = field("month")?.to_owned();
        let total = field("total_cents")?;
        if lines.next().is_some() {
            return Err(malformed());
        }

        // The format holds any token here; the schema holds only a `u64`.
        let total_cents = total
            .parse()
            .map_err(|_| DecodeError::Data(format!("`{total}` is not a total in cents")))?;
        Ok(Report { month, total_cents })
    }

    fn encode(report: &Report, out: &mut impl Write) -> std::io::Result<()> {
        write!(
            out,
            "month: {}\ntotal_cents: {}\n",
            report.month, report.total_cents
        )
    }
}

/// The two ways this service reads and serves a stored report.
type StoredReport = (Json<Report>, Encoded<ReportLines, Report>);

/// Stores a report sent in the line format, and answers it in whichever of the
/// two formats the client accepts.
///
/// `curl -X PUT -H 'content-type: application/vnd.example.report-lines'
/// --data-binary $'month: 2026-10\ntotal_cents: ten\n'` is the 422: the lines
/// are the format, and `ten` is not a `u64`. Dropping a line is the 400.
#[kynos::put("/reports/{month}")]
async fn put_report(
    Path(path): Path<ReportPath>,
    accept: Accept<StoredReport>,
    report: Encoded<ReportLines, Report>,
) -> Result<Negotiated<StoredReport>, NegotiationRejection> {
    // The path names the report, whatever month the body states.
    let report = Report {
        month: path.month,
        ..report.into_inner()
    };

    accept.respond_with(
        &report,
        (
            |report: &Report| Json(report.clone()),
            |report: &Report| Encoded::new(report.clone()),
        ),
    )
}

/// A program generic over what it offers.
///
/// This is why the traits are public rather than merely sealed. The bound is
/// writable, so a helper can be generic over an offer — and it is still closed,
/// so every offer is one whose `content` entry Kynos writes.
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

    let router =
        Router::<()>::new().mount(kynos::routes![get_report, get_versioned_report, put_report]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
