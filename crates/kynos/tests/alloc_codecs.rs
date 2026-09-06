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
    //! The instrument every module below shares: one request builder, and one
    //! counted poll.
    //!
    //! Two items of its own rather than the `support/` module the behavioural
    //! targets share. `support::Pending::call` is an `async fn` that allocates
    //! per request, so it cannot sit inside a region; and it names `Json`
    //! unconditionally, so it cannot be built with `json` off — which is
    //! exactly the build the `form`, `protobuf` and `compression` modules below
    //! have to be measurable in.

    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use alloc_counter::count_alloc;
    use kynos::{
        http::{HeaderValue, Method, Request, StatusCode, body::Body, header},
        router::service::Service,
    };

    /// One measured operation: how it reads in a failure, how its request is
    /// built, the status it has to answer with for the count to be of the
    /// operation, and what it costs today.
    ///
    /// A `fn() -> Request` rather than a built request, because a request is
    /// consumed by the call it drives and every table below is read twice —
    /// once for the record, once for the replay.
    pub(crate) type Measured = (&'static str, fn() -> Request, StatusCode, usize);

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
    pub(crate) fn counted<C>(
        service: &Service<C>,
        request: Request,
        expected: StatusCode,
    ) -> usize {
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

        drop(response);
        allocations
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

    use crate::harness::{Measured, counted, request};

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
    /// and the number the failure reported transcribed into the row. Raising
    /// one is a change to [`nfr.md`](../../../docs/nfr.md#extraction); lowering
    /// one is what a cheaper codec looks like.
    ///
    /// **Decoding this body costs exactly what reading it undecoded costs**,
    /// which is the reading the transport floor exists to make visible. `serde`
    /// deserializes an all-integer struct straight out of the borrowed octets,
    /// so what the operation pays over the bodyless floor is the collection of
    /// the body rather than the codec — and a table with only the bodyless
    /// floor in it would have reported that one allocation as JSON's price.
    /// Writing is the expensive direction: `serde_json` builds the octets in a
    /// buffer of its own before a status is committed.
    const RECORDED: [Measured; 5] = [
        ("POST /floor", floor_request, StatusCode::NO_CONTENT, 7),
        (
            "POST /floor/bytes",
            transport_request,
            StatusCode::NO_CONTENT,
            8,
        ),
        (
            "GET /floor/out",
            responding_floor_request,
            StatusCode::NO_CONTENT,
            7,
        ),
        ("POST /json", decode_request, StatusCode::NO_CONTENT, 8),
        ("GET /json/out", encode_request, StatusCode::OK, 12),
    ];

    /// The record: what each operation of this service costs today.
    ///
    /// Every row is measured before anything is asserted, so a failure reports
    /// the whole table rather than the first row over its ceiling — which is
    /// also what makes reading a fresh set of numbers one run rather than five.
    #[test]
    fn the_operations_cost_what_is_recorded() {
        let service = service();
        let mut over = Vec::new();

        for (operation, build, expected, ceiling) in RECORDED {
            let counted = counted(&service, build(), expected);
            if counted > ceiling {
                over.push(format!(
                    "{operation} allocated {counted}, recorded {ceiling}"
                ));
            }
        }

        assert!(
            over.is_empty(),
            "{over:?}; raising a ceiling is a change to docs/nfr.md, and \
             lowering one is what a cheaper codec looks like"
        );
    }

    /// The relation the request-direction ceilings are there to hold, and the
    /// one that survives a change to any of them.
    ///
    /// Both halves are needed. Costing more than the bodyless floor says the
    /// operation read a body at all; costing at least what reading the same
    /// octets undecoded costs says the codec ran on top of that read rather
    /// than instead of it. The two are equal today — `serde` deserializes an
    /// all-integer struct out of the borrowed octets and owns nothing — so the
    /// second half is what would catch a codec that started skipping the read.
    #[test]
    fn decoding_a_body_costs_more_than_reading_the_same_octets() {
        let service = service();

        let floor = counted(&service, floor_request(), StatusCode::NO_CONTENT);
        let transport = counted(&service, transport_request(), StatusCode::NO_CONTENT);
        let decoding = counted(&service, decode_request(), StatusCode::NO_CONTENT);

        assert!(
            decoding > floor,
            "decoding a JSON body ({decoding}) should cost more than the \
             bodyless operation beside it ({floor})"
        );
        assert!(
            decoding >= transport,
            "decoding a JSON body ({decoding}) should cost at least what \
             reading the same octets undecoded costs ({transport}); a codec \
             cheaper than the transport under it is a codec that did not run"
        );
    }

    /// The responding half of the same relation.
    #[test]
    fn writing_a_body_costs_more_than_the_status_alone() {
        let service = service();

        let floor = counted(&service, responding_floor_request(), StatusCode::NO_CONTENT);
        let encoding = counted(&service, encode_request(), StatusCode::OK);

        assert!(
            encoding > floor,
            "serializing a JSON body ({encoding}) should cost more than the \
             bodyless response beside it ({floor})"
        );
    }

    /// The leak check: whatever an operation costs, the thousandth request
    /// costs the same. A count that climbed would be state accumulating in the
    /// codec, which no single-request measurement can see.
    ///
    /// Every operation is replayed rather than the codec's alone: a table of
    /// five numbers that replayed one would leave four resting on a single
    /// reading.
    #[test]
    fn a_replayed_request_costs_what_the_first_one_did() {
        let service = service();

        for (operation, build, expected, _) in RECORDED {
            let first = counted(&service, build(), expected);
            let mut moved = Vec::new();

            for index in 0..1_000 {
                let counted = counted(&service, build(), expected);
                if counted != first {
                    moved.push((index, counted));
                }
            }

            assert!(
                moved.is_empty(),
                "{operation} allocated {first} times on one request and \
                 differently on {} of the next thousand, starting at {:?}; a \
                 count that moves between identical requests is state \
                 accumulating in the codec",
                moved.len(),
                moved.first()
            );
        }
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

    use crate::harness::{Measured, counted, request};

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
    const RECORDED: [Measured; 5] = [
        ("POST /floor", floor_request, StatusCode::NO_CONTENT, 7),
        (
            "POST /floor/bytes",
            transport_request,
            StatusCode::NO_CONTENT,
            8,
        ),
        (
            "GET /floor/out",
            responding_floor_request,
            StatusCode::NO_CONTENT,
            7,
        ),
        ("POST /form", decode_request, StatusCode::NO_CONTENT, 8),
        ("GET /form/out", encode_request, StatusCode::OK, 13),
    ];

    /// The record: what each operation of this service costs today.
    #[test]
    fn the_operations_cost_what_is_recorded() {
        let service = service();
        let mut over = Vec::new();

        for (operation, build, expected, ceiling) in RECORDED {
            let counted = counted(&service, build(), expected);
            if counted > ceiling {
                over.push(format!(
                    "{operation} allocated {counted}, recorded {ceiling}"
                ));
            }
        }

        assert!(
            over.is_empty(),
            "{over:?}; raising a ceiling is a change to docs/nfr.md, and \
             lowering one is what a cheaper codec looks like"
        );
    }

    /// The relation the request-direction ceilings are there to hold.
    #[test]
    fn decoding_a_body_costs_more_than_reading_the_same_octets() {
        let service = service();

        let floor = counted(&service, floor_request(), StatusCode::NO_CONTENT);
        let transport = counted(&service, transport_request(), StatusCode::NO_CONTENT);
        let decoding = counted(&service, decode_request(), StatusCode::NO_CONTENT);

        assert!(
            decoding > floor,
            "decoding a form body ({decoding}) should cost more than the \
             bodyless operation beside it ({floor})"
        );
        assert!(
            decoding >= transport,
            "decoding a form body ({decoding}) should cost at least what \
             reading the same octets undecoded costs ({transport}); a codec \
             cheaper than the transport under it is a codec that did not run"
        );
    }

    /// The responding half of the same relation.
    #[test]
    fn writing_a_body_costs_more_than_the_status_alone() {
        let service = service();

        let floor = counted(&service, responding_floor_request(), StatusCode::NO_CONTENT);
        let encoding = counted(&service, encode_request(), StatusCode::OK);

        assert!(
            encoding > floor,
            "encoding a form body ({encoding}) should cost more than the \
             bodyless response beside it ({floor})"
        );
    }

    /// The leak check, over every operation this service holds.
    #[test]
    fn a_replayed_request_costs_what_the_first_one_did() {
        let service = service();

        for (operation, build, expected, _) in RECORDED {
            let first = counted(&service, build(), expected);
            let mut moved = Vec::new();

            for index in 0..1_000 {
                let counted = counted(&service, build(), expected);
                if counted != first {
                    moved.push((index, counted));
                }
            }

            assert!(
                moved.is_empty(),
                "{operation} allocated {first} times on one request and \
                 differently on {} of the next thousand, starting at {:?}; a \
                 count that moves between identical requests is state \
                 accumulating in the codec",
                moved.len(),
                moved.first()
            );
        }
    }
}
