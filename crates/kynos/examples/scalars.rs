//! The types whose whole contribution to a description is a `format`.
//!
//! Run it with every scalar backend on at once:
//!
//! ```text
//! cargo run -p kynos --example scalars --features \
//!   uuid,time-chrono,time-jiff,decimal-rust,decimal-big
//! ```
//!
//! An application picks *one* date backend and *one* decimal backend. Both of
//! each appear here because the point of the file is the mapping, and the
//! mapping is only visible when the alternatives sit side by side. The
//! normative table is [`docs/schema.md`](../../../docs/schema.md).
//!
//! Three things are worth noticing:
//!
//! * **The type makes the claim.** There is no `#[schema(format = "uuid")]`,
//!   and the derive rejects one. `format` says what a value *is*, which only
//!   the type can know — a `String` annotated as a UUID is a `String` that
//!   parses anything, and the annotation is a promise the code does not keep.
//! * **Each backend is a feature, and none is on by default.** A description of
//!   a date needs a date library, and choosing one for an application would be
//!   the sort of prescription this framework avoids. What it does prescribe is
//!   the shape both map onto, so `date-time-local` cannot mean two things.
//! * **The offset-less types are not `date-time`.** `NaiveDateTime` and
//!   `civil::DateTime` carry no offset, so they are not RFC 3339 timestamps,
//!   and saying otherwise would document a request body the service rejects.
//!   They take the registered `date-time-local` instead. The demonstration is
//!   in `docs/schema.md`; the assertion is in `schema/tests.rs`.
//!
//! Enabling a backend's own `serde` feature is the application's job. Kynos
//! depends on each library without it, because a schema is derived from the
//! type rather than from the encoder.

use std::net::Ipv4Addr;

use kynos::{prelude::*, server::Server};
use serde::{Deserialize, Serialize};

/// A booking, as a consumer sees it.
///
/// Everything here is a scalar with a registered format, and not one of them is
/// annotated.
#[derive(Schema, Serialize, Deserialize)]
struct Booking {
    /// `string` / `uuid`. The only identifier type Kynos describes, because it
    /// is the only one with a format to claim.
    id: uuid::Uuid,

    /// `string` / `date-time`. An instant carries an offset by construction, so
    /// it is an RFC 3339 timestamp and nothing has to check that it is.
    booked_at: chrono::DateTime<chrono::Utc>,

    /// `string` / `date-time`, from the other backend. The same shape: an
    /// operation that took a `Timestamp` where another took a `DateTime<Utc>`
    /// would emit the same schema, which is what makes the two interchangeable
    /// to a consumer.
    confirmed_at: jiff::Timestamp,

    /// `string` / `date-time-zoned`, plus a `pattern`.
    ///
    /// The one type in either backend with no registered format. RFC 9557
    /// appends the IANA zone in brackets — `2026-08-11T09:00:00-04:00[America/New_York]`
    /// — and that suffix is exactly what stops it being a valid `date-time`. A
    /// consumer that has not heard of the format falls back to `type` and the
    /// pattern, which the specification requires and which loses nothing.
    starts_at: jiff::Zoned,

    /// `string` / `decimal`, carried as a *string*.
    ///
    /// A JSON number would round-trip through an `f64` in most consumers and
    /// lose exactly the precision a decimal exists to keep. Both backends
    /// serialize to a string by default, and the schema follows serde rather
    /// than the other way round.
    price: rust_decimal::Decimal,

    /// `string` / `decimal`, from the arbitrary-precision backend.
    ///
    /// `rust_decimal` is fixed at 28 significant digits, which is right for
    /// money and wrong for a measurement. Both are supported because they are
    /// not rivals.
    exchange_rate: bigdecimal::BigDecimal,

    /// `integer` / `uint8`, with `minimum: 0`.
    ///
    /// The OAI Format Registry names every width in both signednesses, so a
    /// `u8` is a `uint8` rather than an `int32` wide enough to hold one. A
    /// consumer that does not recognise the format still sees `integer` and the
    /// bounds.
    seats: u8,
}

/// When a service is bookable, in the venue's own terms.
///
/// These are the offset-less types. A venue opens at nine in the morning
/// wherever it is, which is a different fact from a moment in time, and the two
/// have different formats because they are different claims.
#[derive(Schema, Serialize, Deserialize)]
struct Availability {
    /// `string` / `date`. Both backends agree, and so does JSON Schema.
    on: jiff::civil::Date,

    /// `string` / `time-local`. Registered, and not the same as `time`: the
    /// latter requires an offset.
    opens_at: jiff::civil::Time,

    /// `string` / `date-time-local`, from the chrono side.
    last_reviewed: chrono::NaiveDateTime,

    /// `string` / `duration`, the ISO 8601 form — `PT1H30M`.
    ///
    /// This is the half of the temporal surface chrono cannot match:
    /// `TimeDelta` serializes as a `[seconds, nanos]` array, so it is
    /// deliberately undescribable and `docs/schema.md` gives the remedy.
    slot_length: jiff::Span,
}

/// What `/bookings/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct BookingPath {
    /// A path parameter is a scalar too, and takes the same format.
    id: uuid::Uuid,
}

/// Fetches one booking.
#[kynos::get("/bookings/{id}")]
async fn get_booking(Path(path): Path<BookingPath>) -> Json<Booking> {
    let _ = path;
    todo!("the router is still a skeleton; this example exists to typecheck")
}

/// Reports when a service can be booked.
#[kynos::get("/availability")]
async fn get_availability() -> Json<Availability> {
    todo!("the router is still a skeleton; this example exists to typecheck")
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![get_booking, get_availability]);

    // Every `format` in this file appears in here, and none of them was written
    // down twice.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
