//! What each opt-in payload codec adds to an operation that mounts it.
//!
//! The allocation-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy), for the shape
//! that document calls an *opt-in payload codec*: a body extractor or response
//! codec behind a feature, which owes "a binary delta, and an allocation count
//! on an operation that names it" and does not owe "a measurement on a route
//! that never mounts it". This file is the second of those two, and only that
//! one — the binary delta is not here.
//!
//! **A second target rather than a second module of [`alloc.rs`](alloc.rs).** A
//! `#[global_allocator]` is process-wide and each integration target is one
//! process, so a counted fixture added beside the routing one would be a
//! fixture that slows it. Two targets is the only arrangement under which
//! neither pays for the other.
//!
//! **Every number here is a difference between two operations of one built
//! service.** Each codec below builds its own, holding its own floors beside
//! the operation that names the codec, so no reading depends on which other
//! features this build has on: a shared service's route set would change with
//! the feature set, and the same ceiling would then mean two different things
//! under `mise run test` and `mise run test:baseline`.
//!
//! **Two floors are measured in the request direction, not one.** A bodyless
//! operation, and one that reads the same octets as
//! [`Binary<OctetStream>`](kynos::extract::body::binary::Binary) and drops
//! them. Against the bodyless floor alone, "reads a body" and "decodes it"
//! would be charged to the codec together — the unattributed delta
//! [`nfr.md`](../../../docs/nfr.md#extraction) warns about one level up.
//!
//! **The ceilings are measurements rather than targets**, which is what
//! [`nfr.md`](../../../docs/nfr.md#thresholds) requires of a first one. Each was
//! read by setting it to zero, running the target, and transcribing the count
//! the failure reported. The relations beside them are asserted from fresh
//! counts and name no ceiling, because
//! [`performance.md`](../../../docs/performance.md#the-taxonomy) says relations
//! outlive absolutes: a toolchain bump that moves every number leaves every
//! relation standing.
//!
//! What this file deliberately does not restate is `alloc.rs`'s
//! `work_on_another_thread_is_not_counted`. That assertion is about
//! `alloc_counter` itself — that a region reports the measuring thread's work
//! and nothing else — and it is as true in this binary as in that one.
//! Asserting it twice would make it read as a property of a fixture rather than
//! of the counter.

#![cfg(feature = "macros")]

/// Declared here rather than reached for, for the reason [`alloc.rs`](alloc.rs)
/// gives: `alloc_counter` installs nothing on its own behalf, so this line is
/// the whole of what puts the counter in this binary.
///
/// Gated with the harness below, because a build with `macros` on and every
/// codec off has nothing to count and no reason to carry a counting allocator.
///
/// There is a line to reach for once #111 lands — `support/counting.rs` carries
/// the same static — and folding this one onto it is #133.
#[cfg(any(
    feature = "json",
    feature = "form",
    feature = "multipart",
    feature = "protobuf",
    feature = "compression"
))]
#[global_allocator]
static ALLOCATOR: alloc_counter::AllocCounterSystem = alloc_counter::AllocCounterSystem;

#[cfg(any(
    feature = "json",
    feature = "form",
    feature = "multipart",
    feature = "protobuf",
    feature = "compression"
))]
mod harness {
    //! The instrument every module below shares: one request builder, two
    //! counted polls — one asserting a status, one a status and the coding the
    //! response carries — and the four assertion bodies every body codec makes
    //! about its own table.
    //!
    //! Items of its own, and `support/mod.rs` is not the reason: its
    //! `Pending::call` is an `async fn` that allocates per request, so it could
    //! never sit inside a region, and it names `Json` unconditionally, so it
    //! could never be built with `json` off — the build the `form`, `protobuf`
    //! and `compression` modules below have to be measurable in. It was never a
    //! candidate.
    //!
    //! The module this one does overlap is `support/counting.rs`, which #111
    //! adds for precisely this purpose — a second counting target including it
    //! with `#[path]` — and which already carries a `#[global_allocator]`, a
    //! by-hand single-poll driver and a request builder: the three items
    //! re-declared here. They are re-declared because that file is on another
    //! branch and these two lanes were cut in parallel, and because the shapes
    //! have still to be reconciled — `counting::counted` drives a `GET` with no
    //! body and drops the response, where a codec measurement builds a method,
    //! a content type and a body, asserts the status the count is of, and reads
    //! the response back. Whichever of the two lands second folds this module
    //! onto that one; #133 is where that is recorded.

    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use alloc_counter::count_alloc;
    use kynos::{
        http::{HeaderValue, Method, Request, Response, StatusCode, body::Body, header},
        router::service::Service,
    };

    /// One measured operation: how it reads in a failure, how its request is
    /// built, the status it has to answer with for the count to be of the
    /// operation, and what it costs today.
    ///
    /// A `fn() -> Request` rather than a built request, because a request is
    /// consumed by the call it drives and every table below is read twice —
    /// once for the record, once for the replay.
    ///
    /// Gated more narrowly than the module around it: the four body codecs
    /// share this shape and `compression` does not, since its table is keyed by
    /// coding and body size instead. A build carrying `compression` alone would
    /// otherwise compile an alias nothing names.
    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    pub(crate) type Measured = (&'static str, fn() -> Request, StatusCode, usize);

    /// The five operations a body codec's module measures, named rather than
    /// ordered.
    ///
    /// The shared bodies below read the rows they need out of one of these. A
    /// `[Measured; 5]` read by position would do as much and would let a table
    /// list its transport floor second by accident; naming the rows is what
    /// makes "the operation this codec's delta is taken against" a thing the
    /// compiler checks rather than a convention four modules keep.
    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    pub(crate) struct Table {
        /// Dispatch, with no body extractor at all.
        pub(crate) bodyless_floor: Measured,
        /// The same octets the codec is handed, read and dropped undecoded.
        pub(crate) transport_floor: Measured,
        /// A status, and no body to write.
        pub(crate) responding_floor: Measured,
        /// The operation that names the codec on the way in.
        pub(crate) decoding: Measured,
        /// The operation that names it on the way out.
        pub(crate) encoding: Measured,
    }

    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    impl Table {
        /// Every row, in the order a failure should read them.
        fn rows(&self) -> [Measured; 5] {
            [
                self.bodyless_floor,
                self.transport_floor,
                self.responding_floor,
                self.decoding,
                self.encoding,
            ]
        }
    }

    /// Builds one request, always outside a counted region.
    ///
    /// Parsing a target, boxing a body and interning a field value are the
    /// caller's cost rather than the operation's — the line
    /// [`alloc.rs`](alloc.rs) draws, for its reason. A `&'static [u8]` body is
    /// what makes that true of the body too: the octets are in the binary, so
    /// wrapping them copies nothing.
    pub(crate) fn request(
        method: Method,
        target: &str,
        content_type: Option<&'static str>,
        body: &'static [u8],
    ) -> Request {
        let mut request = Request::new(if body.is_empty() {
            Body::empty()
        } else {
            Body::from_bytes(bytes::Bytes::from_static(body))
        });

        *request.method_mut() = method;
        *request.uri_mut() = target.parse().expect("a usable request target");

        if let Some(content_type) = content_type {
            request
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        }

        request
    }

    /// Drives one request and reports the heap operations serving it made.
    ///
    /// Fresh allocations and reallocations both, so that growing a buffer
    /// cannot pass as free.
    ///
    /// **The status is asserted, and that is what keeps a number
    /// attributable.** A codec handed a body it declines answers 415 before a
    /// byte is decoded, at a fraction of what decoding costs; recorded
    /// unchecked, that would read as a cheap codec rather than as a fixture
    /// that never reached one.
    ///
    /// The future is polled by hand rather than driven by a runtime, for
    /// [`alloc.rs`](alloc.rs)'s reason: an executor running on the measuring
    /// thread is counted along with the work it drives. Nothing in these
    /// fixtures touches a socket, timer or task — a request body is octets
    /// already in memory, and an encoder reads its input through an
    /// `io::Cursor` — so every future here is ready on its first poll, and the
    /// panic below says so rather than assuming it.
    ///
    /// The response is handed back rather than dropped here, so that a caller
    /// with more to say about it than its status can say it before the drop —
    /// which is outside the region either way.
    fn driven<C>(
        service: &Service<C>,
        request: Request,
        expected: StatusCode,
    ) -> (usize, Response) {
        // Before the region: naming the operation is the report's cost, not the
        // operation's.
        let operation = format!("{} {}", request.method(), request.uri().path());

        let ((allocations, reallocations, _), polled) = count_alloc(|| {
            let mut future = pin!(service.call(request));
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
        });
        let allocations = allocations + reallocations;

        let Poll::Ready(response) = polled else {
            panic!(
                "{operation} was not ready on its first poll; these fixtures \
                 reach no socket, timer or task, so a pending future means a \
                 codec now needs a runtime — and the count above stopped \
                 measuring the whole of one request"
            );
        };

        assert_eq!(
            response.status(),
            expected,
            "{operation} answered {} rather than the {expected} this measurement \
             is of; a request a codec declined never reached the codec, and its \
             count records the refusal instead",
            response.status()
        );

        (allocations, response)
    }

    /// What one request cost, on an operation whose response carries no coding
    /// to check.
    ///
    /// Gated to the body codecs: their services mount nothing that could set a
    /// `Content-Encoding`, and every reading the `compression` module takes has
    /// one to assert. A build carrying `compression` alone would otherwise
    /// compile a function nothing calls.
    ///
    /// Private, because the four measurements a body codec takes are the four
    /// below and no module reaches past them.
    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    fn counted<C>(service: &Service<C>, request: Request, expected: StatusCode) -> usize {
        let (allocations, response) = driven(service, request, expected);

        drop(response);
        allocations
    }

    /// What one request cost, on an operation whose response has to carry
    /// `coding` — or, for `None`, no `Content-Encoding` at all.
    ///
    /// **The coding is asserted for the same reason the status is.** A count is
    /// of an encoder only if that encoder ran: a request asking for `br` that
    /// is answered `gzip`, or answered as it was, is a cheaper reading of a
    /// different thing, and every ceiling and relation in the `compression`
    /// module below would go on holding around it.
    #[cfg(feature = "compression")]
    pub(crate) fn counted_carrying<C>(
        service: &Service<C>,
        request: Request,
        expected: StatusCode,
        coding: Option<&str>,
    ) -> usize {
        let (allocations, response) = driven(service, request, expected);

        let carried = response
            .headers()
            .get(header::CONTENT_ENCODING)
            .map(|coding| coding.to_str().expect("a coding spelled in ASCII"));

        assert_eq!(
            carried, coding,
            "the response carried {carried:?} where this measurement is of \
             {coding:?}; a coding negotiated down, or declined, is a count of \
             an encoder that did not run"
        );

        drop(response);
        allocations
    }

    /// The record: what each operation of one codec's service costs today.
    ///
    /// Every row is measured before anything is asserted, so a failure reports
    /// the whole table rather than the first row over its ceiling — which is
    /// also what makes reading a fresh set of numbers one run rather than five.
    ///
    /// One body here rather than one per module. The four body codecs assert
    /// the same four properties of four different tables, and what
    /// [`testing.md`](../../../docs/testing.md#the-allocation) says keeps a
    /// suite affordable is that the same property is not asserted several times
    /// over. What stays with each codec is what differs between them: its
    /// service, its table, and the prose that reads the numbers in it.
    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    pub(crate) fn record<C>(service: &Service<C>, table: &Table) {
        let mut over = Vec::new();

        for (operation, build, expected, ceiling) in table.rows() {
            let counted = counted(service, build(), expected);
            if counted > ceiling {
                over.push(format!(
                    "{operation} allocated {counted}, recorded {ceiling}"
                ));
            }
        }

        assert!(
            over.is_empty(),
            "{over:?}; the ceiling is `RECORDED` in this file, which is where \
             this number lives — the register row that sanctions it is filed by \
             #121 under docs/nfr.md#extraction and is not there yet, so until \
             then raising one is a change to this table alone, and lowering one \
             is what a cheaper codec looks like"
        );
    }

    /// The leak check: whatever an operation costs, the `replays`th request
    /// costs the same. A count that climbed would be state accumulating in the
    /// codec, which no single-request measurement can see.
    ///
    /// Every operation is replayed rather than the codec's alone: a table of
    /// five numbers that replayed one would leave four resting on a single
    /// reading.
    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    pub(crate) fn replay<C>(service: &Service<C>, table: &Table, replays: usize) {
        for (operation, build, expected, _) in table.rows() {
            let first = counted(service, build(), expected);
            let mut moved = Vec::new();

            for index in 0..replays {
                let counted = counted(service, build(), expected);
                if counted != first {
                    moved.push((index, counted));
                }
            }

            assert!(
                moved.is_empty(),
                "{operation} allocated {first} times on one request and \
                 differently on {} of the next {replays}, starting at {:?}; a \
                 count that moves between identical requests is state \
                 accumulating in the codec",
                moved.len(),
                moved.first()
            );
        }
    }

    /// The relation the request-direction ceilings are there to hold, and the
    /// one that survives a change to any of them.
    ///
    /// Both halves are needed. Costing more than the bodyless floor says the
    /// operation read a body at all; costing at least what reading the same
    /// octets undecoded costs says the codec ran on top of that read rather
    /// than instead of it.
    ///
    /// The three counts are handed back, in the order they were taken, so that
    /// a codec whose currency is owned values can assert the strict form of the
    /// second half on these readings rather than on three more of its own.
    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    pub(crate) fn decoding_costs_more_than_the_read<C>(
        service: &Service<C>,
        table: &Table,
    ) -> (usize, usize, usize) {
        let (bodyless_operation, bodyless_request, bodyless_status, _) = table.bodyless_floor;
        let (transport_operation, transport_request, transport_status, _) = table.transport_floor;
        let (decoding_operation, decoding_request, decoding_status, _) = table.decoding;

        let bodyless = counted(service, bodyless_request(), bodyless_status);
        let transport = counted(service, transport_request(), transport_status);
        let decoding = counted(service, decoding_request(), decoding_status);

        assert!(
            decoding > bodyless,
            "{decoding_operation} allocated {decoding}, where the bodyless \
             operation beside it ({bodyless_operation}) allocated {bodyless}; \
             an operation that decodes a body should cost more than one that \
             reads none"
        );
        assert!(
            decoding >= transport,
            "{decoding_operation} allocated {decoding}, where reading the same \
             octets undecoded ({transport_operation}) allocated {transport}; a \
             codec cheaper than the transport under it is a codec that did not \
             run"
        );

        (bodyless, transport, decoding)
    }

    /// The responding half of the same relation.
    #[cfg(any(
        feature = "json",
        feature = "form",
        feature = "multipart",
        feature = "protobuf"
    ))]
    pub(crate) fn writing_costs_more_than_the_status<C>(service: &Service<C>, table: &Table) {
        let (floor_operation, floor_request, floor_status, _) = table.responding_floor;
        let (encoding_operation, encoding_request, encoding_status, _) = table.encoding;

        let floor = counted(service, floor_request(), floor_status);
        let encoding = counted(service, encoding_request(), encoding_status);

        assert!(
            encoding > floor,
            "{encoding_operation} allocated {encoding}, where the bodyless \
             response beside it ({floor_operation}) allocated {floor}; an \
             operation that writes a body should cost more than one that writes \
             none"
        );
    }
}

/// What `application/json` costs the operations that name it, both directions.
#[cfg(feature = "json")]
mod json {
    use kynos::{
        extract::{body::binary::Binary, media::OctetStream},
        http::{Method, Request, StatusCode},
        prelude::*,
        router::service::Service,
    };

    use crate::harness::{self, Table, request};

    /// The payload both directions carry.
    ///
    /// All-integer, so what is counted is the codec rather than the `String` a
    /// field of any other type would own on its way in and out.
    #[derive(Schema, serde::Deserialize, serde::Serialize)]
    struct Reading {
        id: u64,
        value: u64,
    }

    /// One `Reading`, as the octets a client sends.
    ///
    /// The transport floor reads exactly these octets as
    /// `Binary<OctetStream>`, so the difference between the two operations is
    /// the decode and nothing else. The two requests declare different media
    /// types, but a `Content-Type` is written before the region opens and
    /// compared without allocating inside it.
    const BODY: &[u8] = br#"{"id":7,"value":11}"#;

    /// The bodyless floor: dispatch, and no body extractor at all.
    #[kynos::post("/floor")]
    async fn floor() -> NoContent {
        NoContent
    }

    /// The transport floor: the same octets, read and dropped undecoded.
    #[kynos::post("/floor/bytes")]
    async fn floor_bytes(body: Binary<OctetStream>) -> NoContent {
        drop(body.into_inner());
        NoContent
    }

    /// The responding floor: a status, and no body to write.
    #[kynos::get("/floor/out")]
    async fn floor_out() -> NoContent {
        NoContent
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/json")]
    async fn decode(Json(reading): Json<Reading>) -> NoContent {
        let _ = reading;
        NoContent
    }

    /// The operation that names it on the way out.
    #[kynos::get("/json/out")]
    async fn encode() -> Json<Reading> {
        Json(Reading { id: 7, value: 11 })
    }

    fn service() -> Service<()> {
        Router::<()>::new()
            .mount(kynos::routes![
                floor,
                floor_bytes,
                floor_out,
                decode,
                encode
            ])
            .build(())
            .expect("a describable router")
    }

    fn floor_request() -> Request {
        request(Method::POST, "/floor", None, b"")
    }

    fn transport_request() -> Request {
        request(
            Method::POST,
            "/floor/bytes",
            Some("application/octet-stream"),
            BODY,
        )
    }

    fn responding_floor_request() -> Request {
        request(Method::GET, "/floor/out", None, b"")
    }

    fn decode_request() -> Request {
        request(Method::POST, "/json", Some("application/json"), BODY)
    }

    fn encode_request() -> Request {
        request(Method::GET, "/json/out", None, b"")
    }

    /// Every operation this service holds, and what one request to it costs
    /// today.
    ///
    /// Read rather than chosen: each ceiling was set to zero, the target run,
    /// and the number the failure reported transcribed into the row. This table
    /// is where the number lives; the register row that sanctions it is filed
    /// by #121 under [`nfr.md`](../../../docs/nfr.md#extraction). Raising one is
    /// a change to both once that lands; lowering one is what a cheaper codec
    /// looks like.
    ///
    /// **Decoding this body costs exactly what reading it undecoded costs**,
    /// which is the reading the transport floor exists to make visible. `serde`
    /// deserializes an all-integer struct straight out of the borrowed octets,
    /// so what the operation pays over the bodyless floor is the collection of
    /// the body rather than the codec — and a table with only the bodyless
    /// floor in it would have reported that one allocation as JSON's price.
    /// Writing is the expensive direction: `serde_json` builds the octets in a
    /// buffer of its own before a status is committed.
    const RECORDED: Table = Table {
        bodyless_floor: ("POST /floor", floor_request, StatusCode::NO_CONTENT, 7),
        transport_floor: (
            "POST /floor/bytes",
            transport_request,
            StatusCode::NO_CONTENT,
            8,
        ),
        responding_floor: (
            "GET /floor/out",
            responding_floor_request,
            StatusCode::NO_CONTENT,
            7,
        ),
        decoding: ("POST /json", decode_request, StatusCode::NO_CONTENT, 8),
        encoding: ("GET /json/out", encode_request, StatusCode::OK, 12),
    };

    /// The record: what each operation of this service costs today.
    #[test]
    fn the_operations_cost_what_is_recorded() {
        harness::record(&service(), &RECORDED);
    }

    /// The relation the request-direction ceilings are there to hold.
    ///
    /// The two halves the harness asserts are equal today — `serde`
    /// deserializes an all-integer struct out of the borrowed octets and owns
    /// nothing — so the transport half is the one that would catch a codec that
    /// started skipping the read.
    #[test]
    fn decoding_a_body_costs_more_than_reading_the_same_octets() {
        harness::decoding_costs_more_than_the_read(&service(), &RECORDED);
    }

    /// The responding half of the same relation.
    #[test]
    fn writing_a_body_costs_more_than_the_status_alone() {
        harness::writing_costs_more_than_the_status(&service(), &RECORDED);
    }

    /// The leak check, over every operation this service holds.
    #[test]
    fn a_replayed_request_costs_what_the_first_one_did() {
        harness::replay(&service(), &RECORDED, 1_000);
    }
}

/// What `application/x-www-form-urlencoded` costs the operations that name it,
/// both directions.
#[cfg(feature = "form")]
mod form {
    use kynos::{
        extract::{
            body::{binary::Binary, form::Form},
            media::OctetStream,
        },
        http::{Method, Request, StatusCode},
        prelude::*,
        router::service::Service,
    };

    use crate::harness::{self, Table, request};

    /// The payload both directions carry, in the shape the JSON module uses so
    /// the two codecs are compared on the same value rather than on two.
    #[derive(Schema, serde::Deserialize, serde::Serialize)]
    struct Reading {
        id: u64,
        value: u64,
    }

    /// One `Reading`, as the octets a client sends.
    const BODY: &[u8] = b"id=7&value=11";

    /// The bodyless floor: dispatch, and no body extractor at all.
    #[kynos::post("/floor")]
    async fn floor() -> NoContent {
        NoContent
    }

    /// The transport floor: the same octets, read and dropped undecoded.
    #[kynos::post("/floor/bytes")]
    async fn floor_bytes(body: Binary<OctetStream>) -> NoContent {
        drop(body.into_inner());
        NoContent
    }

    /// The responding floor: a status, and no body to write.
    #[kynos::get("/floor/out")]
    async fn floor_out() -> NoContent {
        NoContent
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/form")]
    async fn decode(Form(reading): Form<Reading>) -> NoContent {
        let _ = reading;
        NoContent
    }

    /// The operation that names it on the way out.
    #[kynos::get("/form/out")]
    async fn encode() -> Form<Reading> {
        Form(Reading { id: 7, value: 11 })
    }

    fn service() -> Service<()> {
        Router::<()>::new()
            .mount(kynos::routes![
                floor,
                floor_bytes,
                floor_out,
                decode,
                encode
            ])
            .build(())
            .expect("a describable router")
    }

    fn floor_request() -> Request {
        request(Method::POST, "/floor", None, b"")
    }

    fn transport_request() -> Request {
        request(
            Method::POST,
            "/floor/bytes",
            Some("application/octet-stream"),
            BODY,
        )
    }

    fn responding_floor_request() -> Request {
        request(Method::GET, "/floor/out", None, b"")
    }

    fn decode_request() -> Request {
        request(
            Method::POST,
            "/form",
            Some("application/x-www-form-urlencoded"),
            BODY,
        )
    }

    fn encode_request() -> Request {
        request(Method::GET, "/form/out", None, b"")
    }

    /// Every operation this service holds, and what one request to it costs
    /// today.
    ///
    /// Read the way the JSON table was read: every ceiling set to zero, the
    /// target run, the counts the failure reported transcribed.
    ///
    /// The request direction reads exactly as JSON's does — `serde_urlencoded`
    /// deserializes an all-integer struct out of the borrowed octets too, so
    /// the operation pays the body's collection and nothing more. The
    /// responding direction costs one allocation more than JSON's: the form
    /// encoder builds a `String` and the response body is then built from it,
    /// where `serde_json` writes into a `Vec<u8>` that becomes the body
    /// directly.
    const RECORDED: Table = Table {
        bodyless_floor: ("POST /floor", floor_request, StatusCode::NO_CONTENT, 7),
        transport_floor: (
            "POST /floor/bytes",
            transport_request,
            StatusCode::NO_CONTENT,
            8,
        ),
        responding_floor: (
            "GET /floor/out",
            responding_floor_request,
            StatusCode::NO_CONTENT,
            7,
        ),
        decoding: ("POST /form", decode_request, StatusCode::NO_CONTENT, 8),
        encoding: ("GET /form/out", encode_request, StatusCode::OK, 13),
    };

    /// The record: what each operation of this service costs today.
    #[test]
    fn the_operations_cost_what_is_recorded() {
        harness::record(&service(), &RECORDED);
    }

    /// The relation the request-direction ceilings are there to hold.
    #[test]
    fn decoding_a_body_costs_more_than_reading_the_same_octets() {
        harness::decoding_costs_more_than_the_read(&service(), &RECORDED);
    }

    /// The responding half of the same relation.
    #[test]
    fn writing_a_body_costs_more_than_the_status_alone() {
        harness::writing_costs_more_than_the_status(&service(), &RECORDED);
    }

    /// The leak check, over every operation this service holds.
    #[test]
    fn a_replayed_request_costs_what_the_first_one_did() {
        harness::replay(&service(), &RECORDED, 1_000);
    }
}

/// What `multipart/form-data` costs the operations that name it, both
/// directions.
///
/// The one codec whose floors do not bound it from below in the way the others'
/// do. Multipart's currency is
/// [`Part`](kynos::extract::body::multipart::Part), which owns a `String` field
/// name — and a declared field owns its value, since `FromPart` is implemented
/// for `String`, `Bytes` and `FilePart` and only the middle one borrows.
/// Building the parts is therefore the codec's own cost rather than a cost a
/// cleverer fixture could measure away, and the payload below is the smallest
/// one the derive accepts: a single field.
#[cfg(feature = "multipart")]
mod multipart {
    use kynos::{
        extract::{
            body::{binary::Binary, multipart::MultipartForm},
            media::OctetStream,
        },
        http::{Method, Request, StatusCode},
        prelude::*,
        router::service::Service,
    };

    use crate::harness::{self, Table, request};

    /// The payload both directions carry.
    ///
    /// One `String` field rather than the all-integer struct the other codecs
    /// measure, because there is no all-integer multipart payload: a part is
    /// octets plus a media type, and the three `FromPart` shapes are `String`,
    /// `Bytes` and `FilePart`. `Bytes` is the one that borrows, and it has no
    /// `Schema`, so a declared field is an owned one.
    #[derive(Schema, kynos::MultipartForm)]
    struct Upload {
        note: String,
    }

    /// One `Upload`, as the octets a client sends.
    ///
    /// RFC 2046 delimiters around one RFC 7578 part. The transport floor reads
    /// exactly these octets as `Binary<OctetStream>`, so the difference is the
    /// parse, the part, and the field conversion.
    const BODY: &[u8] =
        b"--kynos\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nseven\r\n--kynos--\r\n";

    /// The bodyless floor: dispatch, and no body extractor at all.
    #[kynos::post("/floor")]
    async fn floor() -> NoContent {
        NoContent
    }

    /// The transport floor: the same octets, read and dropped unparsed.
    #[kynos::post("/floor/bytes")]
    async fn floor_bytes(body: Binary<OctetStream>) -> NoContent {
        drop(body.into_inner());
        NoContent
    }

    /// The responding floor: a status, and no body to write.
    #[kynos::get("/floor/out")]
    async fn floor_out() -> NoContent {
        NoContent
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/multipart")]
    async fn decode(MultipartForm(upload): MultipartForm<Upload>) -> NoContent {
        drop(upload);
        NoContent
    }

    /// The operation that names it on the way out.
    ///
    /// The `String` is built inside the measured region and counted with the
    /// codec, because a `MultipartForm<T>` that owns nothing does not exist —
    /// see this module's own note.
    #[kynos::get("/multipart/out")]
    async fn encode() -> MultipartForm<Upload> {
        MultipartForm(Upload {
            note: "seven".to_owned(),
        })
    }

    fn service() -> Service<()> {
        Router::<()>::new()
            .mount(kynos::routes![
                floor,
                floor_bytes,
                floor_out,
                decode,
                encode
            ])
            .build(())
            .expect("a describable router")
    }

    fn floor_request() -> Request {
        request(Method::POST, "/floor", None, b"")
    }

    fn transport_request() -> Request {
        request(
            Method::POST,
            "/floor/bytes",
            Some("application/octet-stream"),
            BODY,
        )
    }

    fn responding_floor_request() -> Request {
        request(Method::GET, "/floor/out", None, b"")
    }

    fn decode_request() -> Request {
        request(
            Method::POST,
            "/multipart",
            Some("multipart/form-data; boundary=kynos"),
            BODY,
        )
    }

    fn encode_request() -> Request {
        request(Method::GET, "/multipart/out", None, b"")
    }

    /// Every operation this service holds, and what one request to it costs
    /// today.
    ///
    /// Read the way the JSON table was read: every ceiling set to zero, the
    /// target run, the counts the failure reported transcribed.
    ///
    /// **This is the expensive codec, by roughly an order of magnitude**, and
    /// on a body carrying one part of five octets. Where JSON and the form
    /// codec deserialize out of the borrowed body and add nothing to the read,
    /// multipart walks the octets through a parser holding its own state,
    /// produces an owned `Part` per part — field name, optional file name,
    /// optional media type — and then converts each part into its declared
    /// field. The responding direction pays a comparable bill for the mirror
    /// of that: a delimiter derived from the parts' octets, a header block
    /// rendered per part, and the framing around them.
    const RECORDED: Table = Table {
        bodyless_floor: ("POST /floor", floor_request, StatusCode::NO_CONTENT, 7),
        transport_floor: (
            "POST /floor/bytes",
            transport_request,
            StatusCode::NO_CONTENT,
            8,
        ),
        responding_floor: (
            "GET /floor/out",
            responding_floor_request,
            StatusCode::NO_CONTENT,
            7,
        ),
        decoding: (
            "POST /multipart",
            decode_request,
            StatusCode::NO_CONTENT,
            31,
        ),
        encoding: ("GET /multipart/out", encode_request, StatusCode::OK, 26),
    };

    /// The record: what each operation of this service costs today.
    #[test]
    fn the_operations_cost_what_is_recorded() {
        harness::record(&service(), &RECORDED);
    }

    /// The relation the request-direction ceilings are there to hold.
    ///
    /// Multipart is the codec where the transport floor bites: a parser that
    /// walks the octets, one owned `Part` per part and one field conversion per
    /// declared field all sit above the read, so a count that fell to the
    /// transport floor would mean the body was never parsed.
    ///
    /// The strict form of that half is asserted here rather than in the
    /// harness, on the readings the harness took: it is true of this codec's
    /// currency and of no other's, and the three that deserialize out of the
    /// borrowed octets cost exactly the read.
    #[test]
    fn decoding_a_body_costs_more_than_reading_the_same_octets() {
        let (_, transport, decoding) =
            harness::decoding_costs_more_than_the_read(&service(), &RECORDED);

        assert!(
            decoding > transport,
            "POST /multipart allocated {decoding}, where reading the same \
             octets unparsed allocated {transport}; every part this codec \
             produces owns its name, so there is no body it could decode for \
             what the read alone costs"
        );
    }

    /// The responding half of the same relation.
    #[test]
    fn writing_a_body_costs_more_than_the_status_alone() {
        harness::writing_costs_more_than_the_status(&service(), &RECORDED);
    }

    /// The leak check, over every operation this service holds.
    #[test]
    fn a_replayed_request_costs_what_the_first_one_did() {
        harness::replay(&service(), &RECORDED, 1_000);
    }
}

/// What `application/protobuf` costs the operations that name it, both
/// directions.
#[cfg(feature = "protobuf")]
mod protobuf {
    use kynos::{
        extract::{
            body::{binary::Binary, protobuf::Protobuf},
            media::OctetStream,
        },
        http::{Method, Request, StatusCode},
        prelude::*,
        router::service::Service,
    };

    use crate::harness::{self, Table, request};

    /// The payload both directions carry, in the shape the JSON and form
    /// modules use so the three codecs are compared on the same value.
    ///
    /// Derived twice, for the reason `examples/protobuf.rs` gives at length:
    /// `prost::Message` decides the octets and `Schema` decides what the
    /// description says they mean, and neither is derivable from the other.
    #[derive(Clone, PartialEq, prost::Message, Schema)]
    struct Reading {
        #[prost(uint64, tag = "1")]
        id: u64,
        #[prost(uint64, tag = "2")]
        value: u64,
    }

    /// One `Reading`, as the octets a client sends: two tagged varints.
    const BODY: &[u8] = b"\x08\x07\x10\x0b";

    /// The bodyless floor: dispatch, and no body extractor at all.
    #[kynos::post("/floor")]
    async fn floor() -> NoContent {
        NoContent
    }

    /// The transport floor: the same octets, read and dropped undecoded.
    #[kynos::post("/floor/bytes")]
    async fn floor_bytes(body: Binary<OctetStream>) -> NoContent {
        drop(body.into_inner());
        NoContent
    }

    /// The responding floor: a status, and no body to write.
    #[kynos::get("/floor/out")]
    async fn floor_out() -> NoContent {
        NoContent
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/protobuf")]
    async fn decode(Protobuf(reading): Protobuf<Reading>) -> NoContent {
        let _ = reading;
        NoContent
    }

    /// The operation that names it on the way out.
    #[kynos::get("/protobuf/out")]
    async fn encode() -> Protobuf<Reading> {
        Protobuf(Reading { id: 7, value: 11 })
    }

    fn service() -> Service<()> {
        Router::<()>::new()
            .mount(kynos::routes![
                floor,
                floor_bytes,
                floor_out,
                decode,
                encode
            ])
            .build(())
            .expect("a describable router")
    }

    fn floor_request() -> Request {
        request(Method::POST, "/floor", None, b"")
    }

    fn transport_request() -> Request {
        request(
            Method::POST,
            "/floor/bytes",
            Some("application/octet-stream"),
            BODY,
        )
    }

    fn responding_floor_request() -> Request {
        request(Method::GET, "/floor/out", None, b"")
    }

    fn decode_request() -> Request {
        request(
            Method::POST,
            "/protobuf",
            Some("application/protobuf"),
            BODY,
        )
    }

    fn encode_request() -> Request {
        request(Method::GET, "/protobuf/out", None, b"")
    }

    /// Every operation this service holds, and what one request to it costs
    /// today.
    ///
    /// Read the way the JSON table was read: every ceiling set to zero, the
    /// target run, the counts the failure reported transcribed.
    ///
    /// The cheapest of the four, in both directions. Decoding adds nothing over
    /// the read, as JSON's and the form codec's do on an all-integer payload;
    /// and encoding is the only one of the three that beats JSON, because
    /// `prost` writes the message into one growable buffer and that buffer
    /// becomes the body.
    const RECORDED: Table = Table {
        bodyless_floor: ("POST /floor", floor_request, StatusCode::NO_CONTENT, 7),
        transport_floor: (
            "POST /floor/bytes",
            transport_request,
            StatusCode::NO_CONTENT,
            8,
        ),
        responding_floor: (
            "GET /floor/out",
            responding_floor_request,
            StatusCode::NO_CONTENT,
            7,
        ),
        decoding: ("POST /protobuf", decode_request, StatusCode::NO_CONTENT, 8),
        encoding: ("GET /protobuf/out", encode_request, StatusCode::OK, 11),
    };

    /// The record: what each operation of this service costs today.
    #[test]
    fn the_operations_cost_what_is_recorded() {
        harness::record(&service(), &RECORDED);
    }

    /// The relation the request-direction ceilings are there to hold.
    #[test]
    fn decoding_a_body_costs_more_than_reading_the_same_octets() {
        harness::decoding_costs_more_than_the_read(&service(), &RECORDED);
    }

    /// The responding half of the same relation.
    #[test]
    fn writing_a_body_costs_more_than_the_status_alone() {
        harness::writing_costs_more_than_the_status(&service(), &RECORDED);
    }

    /// The leak check, over every operation this service holds.
    #[test]
    fn a_replayed_request_costs_what_the_first_one_did() {
        harness::replay(&service(), &RECORDED, 1_000);
    }
}

/// How `Compression`'s cost grows with the body it holds.
///
/// The one module here that is not a body codec. It is measured under the same
/// shape because [`performance.md`](../../../docs/performance.md#the-taxonomy)
/// grades it under the same one: an opt-in payload codec owes an allocation
/// count on an operation that names it, and an encoder is a codec that names
/// itself in `Content-Encoding` rather than in `Content-Type`.
///
/// **The axis that matters here is body size, which no other module has.** A
/// body extractor's cost is settled by the shape of the payload; an encoder's
/// is settled by how much of it there is, because the encoder drains its output
/// 8 KiB at a time into a growing buffer. So the table below is codings by
/// sizes, and the relations are about growth rather than about a floor.
///
/// All three codings are measured rather than gzip alone: `encode` boxes
/// brotli's future because its encoder state is kilobytes, so what is counted
/// differs between them in exactly the way a single-coding table would hide.
#[cfg(feature = "compression")]
mod compression {
    use std::sync::LazyLock;

    use kynos::{
        extract::{body::binary::Binary, media::OctetStream},
        http::{HeaderValue, Method, Request, StatusCode, header},
        middleware::compression::Compression,
        prelude::*,
        router::service::Service,
    };

    use crate::harness::{counted_carrying, request};

    /// The octets the fixture serves, one buffer per size.
    ///
    /// Built once and forced before every region, so a handler's whole cost is
    /// a `Bytes` clone — a refcount bump, and no allocation. Structured rather
    /// than constant, for the reason `middleware.rs`'s level fixture gives: a
    /// repeated byte compresses to nearly nothing at every size, and the growth
    /// this module measures would flatten into noise.
    /// Built from [`SIZES`], so the length a row is named for is the length the
    /// operation it names serves.
    static BODIES: LazyLock<[bytes::Bytes; 4]> =
        LazyLock::new(|| SIZES.map(|(_, _, length, _)| octets(length)));

    fn octets(length: usize) -> bytes::Bytes {
        let mut octets = Vec::with_capacity(length);
        let mut index = 0_u32;

        while octets.len() < length {
            octets.extend_from_slice(
                format!("{index:x} the quick brown fox {}\n", index % 97).as_bytes(),
            );
            index += 1;
        }

        octets.truncate(length);
        bytes::Bytes::from(octets)
    }

    #[kynos::get("/bytes/0")]
    async fn empty() -> Binary<OctetStream> {
        Binary::new(BODIES[0].clone())
    }

    #[kynos::get("/bytes/1k")]
    async fn small() -> Binary<OctetStream> {
        Binary::new(BODIES[1].clone())
    }

    #[kynos::get("/bytes/16k")]
    async fn medium() -> Binary<OctetStream> {
        Binary::new(BODIES[2].clone())
    }

    #[kynos::get("/bytes/256k")]
    async fn large() -> Binary<OctetStream> {
        Binary::new(BODIES[3].clone())
    }

    /// The four operations with `Compression` over them.
    fn mounted() -> Service<()> {
        Router::<()>::new()
            .mount(kynos::routes![empty, small, medium, large])
            .intercept(Compression::new())
            .build(())
            .expect("a describable router")
    }

    /// The same four with nothing over them: the literal operation without it.
    fn unmounted() -> Service<()> {
        Router::<()>::new()
            .mount(kynos::routes![empty, small, medium, large])
            .build(())
            .expect("a describable router")
    }

    /// One request, asking for `accept`.
    ///
    /// The field is written before the region opens, like every other part of
    /// building a request here.
    fn asking(accept: &'static str, target: &str) -> Request {
        let mut request = request(Method::GET, target, None, b"");
        request
            .headers_mut()
            .insert(header::ACCEPT_ENCODING, HeaderValue::from_static(accept));
        request
    }

    /// The body sizes measured, in the order they grow: an empty body, and then
    /// a ladder on which each rung is a sixteenth of the next. What target
    /// serves each, how many octets it is, and how many times the leak check
    /// replays it.
    ///
    /// The empty body is not a rung of that ladder and is not meant to be. It
    /// is the column where the encoder declines — `worth_encoding` refuses a
    /// body of no octets even at `min_size` zero — which is why the relation
    /// that bounds growth by the drain starts at 1 KiB, and why the ladder is
    /// three sizes rather than four.
    ///
    /// The replay counts fall as the bodies grow because encoding is real CPU
    /// and this target runs under `llvm-cov` as well as plain: a thousand
    /// brotli passes over 256 KiB would put the file against nextest's
    /// 30-second slow bound rather than against anything it measures. What the
    /// replay is looking for — a count that climbs between identical requests
    /// — is a property of the encoder's state across calls rather than of the
    /// body's size, so it is visible at every size and cheapest at the small
    /// ones.
    const SIZES: [(&str, &str, usize, usize); 4] = [
        ("0", "/bytes/0", 0, 1_000),
        ("1 KiB", "/bytes/1k", 1024, 1_000),
        ("16 KiB", "/bytes/16k", 16 * 1024, 200),
        ("256 KiB", "/bytes/256k", 256 * 1024, 25),
    ];

    /// The three codings, spelled as `Accept-Encoding` spells them.
    const CODINGS: [&str; 3] = ["gzip", "br", "zstd"];

    /// The coding a response has to carry, given what the request asked for and
    /// how many octets the operation serves.
    ///
    /// `None` twice over: for a request that asked for `identity`, which
    /// negotiation answers before the chain runs, and for a body of no octets,
    /// which `worth_encoding` refuses after the response is in hand. Both leave
    /// the response as the handler produced it, and neither writes a
    /// `Content-Encoding`.
    fn carried(accept: &str, length: usize) -> Option<&str> {
        (accept != "identity" && length > 0).then_some(accept)
    }

    /// How many 8 KiB reads `encode`'s drain takes over a body of `length`
    /// octets, at most.
    ///
    /// An upper bound rather than a count: what is drained is the encoded form,
    /// and every body here compresses, so the encoder is emptied in no more
    /// reads than the identity octets would take.
    fn drained(length: usize) -> usize {
        length.div_ceil(8 * 1024)
    }

    /// What one request costs on the service `Compression` is mounted on, by
    /// what the request asked for and by how large the body is.
    ///
    /// The columns are [`SIZES`] in order. The `identity` row is the same
    /// operation with the encoder declining to run: it is what each engaged row
    /// is a delta *from*, and the reason the deltas below are of the encoder
    /// rather than of the interceptor's own indirection.
    ///
    /// Read the way every other table here was read: each ceiling set to zero,
    /// the target run, the counts the failure reported transcribed.
    ///
    /// Three readings are worth naming.
    ///
    /// **A zero-length body costs what identity costs, in every coding.**
    /// `worth_encoding` refuses a body of no octets even at `min_size` zero, so
    /// the encoder never runs and the column is the identity row.
    ///
    /// **1 KiB and 16 KiB cost the same in every coding.** The encoder's output
    /// is drained 8 KiB at a time into a growing `BytesMut`, and both bodies
    /// compress to less than one of those chunks — so the two differ in what
    /// the encoder reads and not in what the drain allocates. The delta moves
    /// again at 256 KiB, which is the reason four sizes are measured rather
    /// than two.
    ///
    /// **Brotli is roughly twice gzip and four times zstd.** Its encoder state
    /// is kilobytes, which is why `encode` boxes its future at all; the same
    /// fact shows up here as the allocations that state costs.
    const RECORDED: [(&str, [usize; 4]); 4] = [
        ("identity", [14, 14, 14, 14]),
        ("gzip", [14, 27, 27, 31]),
        ("br", [14, 42, 42, 48]),
        ("zstd", [14, 21, 21, 24]),
    ];

    /// The same operation at 16 KiB, asked for as `identity`, with no
    /// `Compression` mounted at all.
    ///
    /// The difference between this and the `identity` row is what the erased
    /// interceptor chain costs an operation that carries it, with the encoder
    /// declining to run — a cost `alloc.rs` does not reach and this file
    /// records only in passing, since the per-layer measurement at depth 0/4/8
    /// is its own piece of work. It is measured under the same request as that
    /// row so that the chain is the only thing between the two numbers.
    const UNMOUNTED: usize = 10;

    /// The record: what each request against this fixture costs today.
    #[test]
    fn the_operations_cost_what_is_recorded() {
        LazyLock::force(&BODIES);
        let service = mounted();
        let mut over = Vec::new();

        for (accept, ceilings) in RECORDED {
            for ((size, target, length, _), ceiling) in SIZES.into_iter().zip(ceilings) {
                let counted = counted_carrying(
                    &service,
                    asking(accept, target),
                    StatusCode::OK,
                    carried(accept, length),
                );
                if counted > ceiling {
                    over.push(format!(
                        "{accept} at {size} allocated {counted}, recorded {ceiling}"
                    ));
                }
            }
        }

        let counted = counted_carrying(
            &unmounted(),
            asking("identity", "/bytes/16k"),
            StatusCode::OK,
            None,
        );
        if counted > UNMOUNTED {
            over.push(format!(
                "unmounted at 16 KiB allocated {counted}, recorded {UNMOUNTED}"
            ));
        }

        assert!(
            over.is_empty(),
            "{over:?}; the ceiling is `RECORDED` in this file, which is where \
             this number lives — the register row that sanctions it is filed by \
             #121 under docs/nfr.md#middleware and is not there yet, so until \
             then raising one is a change to this table alone, and lowering one \
             is what a cheaper encoder looks like"
        );
    }

    /// What mounting the interceptor costs the operation that carries it.
    ///
    /// The one reading here that is not a delta within one service, and the
    /// only one that answers "what does the feature cost an operation that
    /// mounts it" in the literal sense the taxonomy asks for. It is one size
    /// rather than four because what it measures — the erased chain around the
    /// handler — does not depend on the body.
    ///
    /// **Both sides ask for `identity`**, so the encoder declines on the
    /// mounted one and the difference is the chain and nothing else. Asking for
    /// a coding the mounted side would encode makes this an assertion that
    /// encoding costs more than not encoding — true, measured twice over by the
    /// table above, and not what this test's name claims.
    #[test]
    fn mounting_compression_costs_the_operation_that_carries_it() {
        LazyLock::force(&BODIES);

        let with = counted_carrying(
            &mounted(),
            asking("identity", "/bytes/16k"),
            StatusCode::OK,
            None,
        );
        let without = counted_carrying(
            &unmounted(),
            asking("identity", "/bytes/16k"),
            StatusCode::OK,
            None,
        );

        assert!(
            with > without,
            "an operation under Compression ({with}) should cost more than the \
             same operation without it ({without}), with the encoder declining \
             on both sides"
        );
    }

    /// A body the encoder declines costs less than one it encodes.
    ///
    /// Both ways of declining are asserted, because they leave by different
    /// doors: negotiation refuses before the chain runs, and `worth_encoding`
    /// refuses after the response is in hand.
    ///
    /// **Every rung of the ladder, rather than 16 KiB alone.** The relation
    /// below guards `engaged >= alone` at every size before it subtracts, which
    /// is the weak half of this same pair; asserted at one size only, the
    /// strict half left 1 KiB and 256 KiB unable to tell an encoder that costs
    /// what declining costs from one that runs. The readings are the ones the
    /// table beside it already takes, so the sweep buys that for nothing.
    ///
    /// The empty column is the *baseline* of the second assertion rather than a
    /// rung of the first. At no octets `worth_encoding` refuses whatever the
    /// request asked for, so the encoder declines on both sides and the two
    /// costs are equal by construction — which is the reading the second
    /// assertion is made against.
    #[test]
    fn a_body_left_alone_costs_less_than_one_encoded() {
        LazyLock::force(&BODIES);
        let service = mounted();
        let (_, empty_target, empty_length, _) = SIZES[0];

        for coding in CODINGS {
            let empty = counted_carrying(
                &service,
                asking(coding, empty_target),
                StatusCode::OK,
                carried(coding, empty_length),
            );

            for (size, target, length, _) in SIZES.into_iter().skip(1) {
                let declined =
                    counted_carrying(&service, asking("identity", target), StatusCode::OK, None);
                let encoded = counted_carrying(
                    &service,
                    asking(coding, target),
                    StatusCode::OK,
                    carried(coding, length),
                );

                assert!(
                    encoded > declined,
                    "{coding} on {size} ({encoded}) should cost more than the \
                     same response the client asked for as identity ({declined})"
                );
                assert!(
                    encoded > empty,
                    "{coding} on {size} ({encoded}) should cost more than \
                     {coding} on a body too small to be worth encoding \
                     ({empty})"
                );
            }
        }
    }

    /// The relation the table exists to hold: the encoder's delta grows with
    /// the body, and no faster than the drain that produces it.
    ///
    /// A delta is taken against the same operation at the same size with
    /// identity asked for, so dispatch, the handler and the interceptor's own
    /// indirection all cancel and what is left is the encode.
    ///
    /// **The subtraction is guarded rather than clamped**, at every size.
    /// Engaging an encoder cannot cost less than declining to, and a
    /// `saturating_sub` over a difference that ran backwards would hand these
    /// relations a zero — which is monotone from below, and under every bound
    /// the drain sets. The reading the encoder declines to make is the one
    /// worth catching, so it is asserted where it is taken rather than
    /// arithmetically erased.
    ///
    /// **The bound is the drain rather than the body.** `encode` empties its
    /// encoder 8 KiB at a time into a growing `BytesMut`, so growing the body
    /// from one measured size to the next buys the encoder at most
    /// [`drained`]`(larger) - 1` further reads, and the delta may grow by at
    /// most one allocation apiece.
    ///
    /// The subtrahend is one rather than [`drained`]`(smaller)`. [`drained`] is
    /// an upper bound on each side, and a difference of two upper bounds bounds
    /// nothing: it is smaller than the largest growth the drain permits
    /// whenever the smaller body drains in fewer reads than its own bound
    /// allows, which is what compression makes likely. What the smaller size is
    /// known to spend is its *minimum* — a body of any octets at all is drained
    /// at least once — so that is what the larger size's bound is measured
    /// against. Bounding growth by the body instead — `delta(16N) <= 16 *
    /// delta(N)`, which is what stood here — is vacuous at these magnitudes: it
    /// allowed 208 allocations where 13 were measured. The drain allows one
    /// between 1 KiB and 16 KiB, because 16 KiB is emptied in at most two reads
    /// and 1 KiB in at least one, and the growth measured there is none.
    ///
    /// The step from the declined column is deliberately not bounded this way.
    /// At zero octets the encoder never runs and the drain never happens, so
    /// what separates that column from 1 KiB is the encoder's own setup — a
    /// constant per response, which the two relations here are not about.
    #[test]
    fn the_encoders_delta_grows_with_the_body_and_no_faster() {
        LazyLock::force(&BODIES);
        let service = mounted();

        for coding in CODINGS {
            let deltas = SIZES.map(|(size, target, length, _)| {
                let engaged = counted_carrying(
                    &service,
                    asking(coding, target),
                    StatusCode::OK,
                    carried(coding, length),
                );
                let alone =
                    counted_carrying(&service, asking("identity", target), StatusCode::OK, None);

                assert!(
                    engaged >= alone,
                    "{coding} at {size} allocated {engaged}, where the same \
                     response left alone allocated {alone}; engaging an encoder \
                     cannot cost less than declining to, so a difference that \
                     runs backwards is two readings of different things rather \
                     than a delta"
                );

                engaged - alone
            });

            for pair in deltas.windows(2) {
                assert!(
                    pair[0] <= pair[1],
                    "{coding} allocated {deltas:?} over {SIZES:?}; a delta that \
                     falls as the body grows is a measurement of something else"
                );
            }

            // From the second size on: the step out of the declined column is
            // the encoder's setup rather than anything its drain explains.
            for larger in 2..SIZES.len() {
                let smaller = larger - 1;
                let allowance = drained(SIZES[larger].2) - 1;
                let growth = deltas[larger] - deltas[smaller];

                assert!(
                    growth <= allowance,
                    "{coding} allocated {deltas:?} over {SIZES:?}; the delta grew \
                     by {growth} from {} to {}, where the drain that produces it \
                     takes at most {allowance} further 8 KiB reads — a delta \
                     outgrowing its drain is a buffer growing by doubling from \
                     nothing on every response, or state kept per octet",
                    SIZES[smaller].0,
                    SIZES[larger].0
                );
            }
        }
    }

    /// The leak check, over every coding at every size.
    ///
    /// An encoder is where accumulating state would be easiest to introduce and
    /// hardest to see: a dictionary kept between responses, or a buffer reused
    /// and grown, would leave every count but the first one different.
    #[test]
    fn a_replayed_request_costs_what_the_first_one_did() {
        LazyLock::force(&BODIES);
        let service = mounted();

        for (accept, _) in RECORDED {
            for (size, target, length, replays) in SIZES {
                let carried = carried(accept, length);
                let first =
                    counted_carrying(&service, asking(accept, target), StatusCode::OK, carried);
                let mut moved = Vec::new();

                for index in 0..replays {
                    let counted =
                        counted_carrying(&service, asking(accept, target), StatusCode::OK, carried);
                    if counted != first {
                        moved.push((index, counted));
                    }
                }

                assert!(
                    moved.is_empty(),
                    "{accept} at {size} allocated {first} times on one request \
                     and differently on {} of the next {replays}, starting at \
                     {:?}; a count that moves between identical requests is \
                     state accumulating in the encoder",
                    moved.len(),
                    moved.first()
                );
            }
        }
    }
}
