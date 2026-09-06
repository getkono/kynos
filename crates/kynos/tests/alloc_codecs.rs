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

    use crate::harness::{Measured, counted, request};

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
        (
            "POST /multipart",
            decode_request,
            StatusCode::NO_CONTENT,
            31,
        ),
        ("GET /multipart/out", encode_request, StatusCode::OK, 26),
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
    ///
    /// Multipart is the codec where the transport floor bites: a parser that
    /// walks the octets, one owned `Part` per part and one field conversion per
    /// declared field all sit above the read, so a count that fell to the
    /// transport floor would mean the body was never parsed.
    #[test]
    fn decoding_a_body_costs_more_than_reading_the_same_octets() {
        let service = service();

        let floor = counted(&service, floor_request(), StatusCode::NO_CONTENT);
        let transport = counted(&service, transport_request(), StatusCode::NO_CONTENT);
        let decoding = counted(&service, decode_request(), StatusCode::NO_CONTENT);

        assert!(
            decoding > floor,
            "decoding a multipart body ({decoding}) should cost more than the \
             bodyless operation beside it ({floor})"
        );
        assert!(
            decoding > transport,
            "decoding a multipart body ({decoding}) should cost more than \
             reading the same octets unparsed ({transport}); every part this \
             codec produces owns its name"
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
            "rendering a multipart body ({encoding}) should cost more than the \
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

    use crate::harness::{Measured, counted, request};

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
        ("POST /protobuf", decode_request, StatusCode::NO_CONTENT, 8),
        ("GET /protobuf/out", encode_request, StatusCode::OK, 11),
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
            "decoding a protobuf body ({decoding}) should cost more than the \
             bodyless operation beside it ({floor})"
        );
        assert!(
            decoding >= transport,
            "decoding a protobuf body ({decoding}) should cost at least what \
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
            "encoding a protobuf body ({encoding}) should cost more than the \
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

    use crate::harness::{counted, request};

    /// The octets the fixture serves, one buffer per size.
    ///
    /// Built once and forced before every region, so a handler's whole cost is
    /// a `Bytes` clone — a refcount bump, and no allocation. Structured rather
    /// than constant, for the reason `middleware.rs`'s level fixture gives: a
    /// repeated byte compresses to nearly nothing at every size, and the growth
    /// this module measures would flatten into noise.
    static BODIES: LazyLock<[bytes::Bytes; 4]> =
        LazyLock::new(|| [0, 1024, 16 * 1024, 256 * 1024].map(octets));

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

    /// The body sizes measured, in the order they grow, each a sixteenth of the
    /// next, and how many times the leak check replays each.
    ///
    /// The replay counts fall as the bodies grow because encoding is real CPU
    /// and this target runs under `llvm-cov` as well as plain: a thousand
    /// brotli passes over 256 KiB would put the file against nextest's
    /// 30-second slow bound rather than against anything it measures. What the
    /// replay is looking for — a count that climbs between identical requests
    /// — is a property of the encoder's state across calls rather than of the
    /// body's size, so it is visible at every size and cheapest at the small
    /// ones.
    const SIZES: [(&str, &str, usize); 4] = [
        ("0", "/bytes/0", 1_000),
        ("1 KiB", "/bytes/1k", 1_000),
        ("16 KiB", "/bytes/16k", 200),
        ("256 KiB", "/bytes/256k", 25),
    ];

    /// The three codings, spelled as `Accept-Encoding` spells them.
    const CODINGS: [&str; 3] = ["gzip", "br", "zstd"];

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
            for ((size, target, _), ceiling) in SIZES.into_iter().zip(ceilings) {
                let counted = counted(&service, asking(accept, target), StatusCode::OK);
                if counted > ceiling {
                    over.push(format!(
                        "{accept} at {size} allocated {counted}, recorded {ceiling}"
                    ));
                }
            }
        }

        let counted = counted(
            &unmounted(),
            asking("identity", "/bytes/16k"),
            StatusCode::OK,
        );
        if counted > UNMOUNTED {
            over.push(format!(
                "unmounted at 16 KiB allocated {counted}, recorded {UNMOUNTED}"
            ));
        }

        assert!(
            over.is_empty(),
            "{over:?}; raising a ceiling is a change to docs/nfr.md, and \
             lowering one is what a cheaper encoder looks like"
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

        let with = counted(&mounted(), asking("identity", "/bytes/16k"), StatusCode::OK);
        let without = counted(
            &unmounted(),
            asking("identity", "/bytes/16k"),
            StatusCode::OK,
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
    #[test]
    fn a_body_left_alone_costs_less_than_one_encoded() {
        LazyLock::force(&BODIES);
        let service = mounted();

        let declined = counted(&service, asking("identity", "/bytes/16k"), StatusCode::OK);

        for coding in CODINGS {
            let encoded = counted(&service, asking(coding, "/bytes/16k"), StatusCode::OK);
            let empty = counted(&service, asking(coding, "/bytes/0"), StatusCode::OK);

            assert!(
                encoded > declined,
                "{coding} on 16 KiB ({encoded}) should cost more than the same \
                 response the client asked for as identity ({declined})"
            );
            assert!(
                encoded > empty,
                "{coding} on 16 KiB ({encoded}) should cost more than {coding} \
                 on a body too small to be worth encoding ({empty})"
            );
        }
    }

    /// The relation the table exists to hold: the encoder's delta grows with
    /// the body, and no faster than the body does.
    ///
    /// A delta is taken against the same operation at the same size with
    /// identity asked for, so dispatch, the handler and the interceptor's own
    /// indirection all cancel and what is left is the encode.
    ///
    /// Each size is a sixteenth of the next, so "at most linear" is
    /// `delta(16N) <= 16 * delta(N)`. That is the shape a buffer drained in
    /// fixed-size chunks has; a delta that outgrew it would be a buffer growing
    /// by doubling from nothing on every response, or state kept per octet.
    #[test]
    fn the_encoders_delta_grows_with_the_body_and_no_faster() {
        LazyLock::force(&BODIES);
        let service = mounted();

        for coding in CODINGS {
            let deltas = SIZES.map(|(_, target, _)| {
                let engaged = counted(&service, asking(coding, target), StatusCode::OK);
                let alone = counted(&service, asking("identity", target), StatusCode::OK);
                engaged.saturating_sub(alone)
            });

            for pair in deltas.windows(2) {
                assert!(
                    pair[0] <= pair[1],
                    "{coding} allocated {deltas:?} over {SIZES:?}; a delta that \
                     falls as the body grows is a measurement of something else"
                );
            }

            for (smaller, larger) in deltas.iter().skip(1).zip(deltas.iter().skip(2)) {
                assert!(
                    *larger <= 16 * *smaller,
                    "{coding} allocated {deltas:?} over {SIZES:?}; a sixteenfold \
                     body should not cost more than sixteen times the \
                     allocations"
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
            for (size, target, replays) in SIZES {
                let first = counted(&service, asking(accept, target), StatusCode::OK);
                let mut moved = Vec::new();

                for index in 0..replays {
                    let counted = counted(&service, asking(accept, target), StatusCode::OK);
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
