//! The fixture the codec sweep builds: one program that *mounts* each codec.
//!
//! Not a teaching example, and not [`fixture.rs`](fixture.rs) either. It is the
//! artifact [`scripts/cost_features.py`] measures under `--kind codec`, which is
//! why it is declared with a `path` rather than left in `examples/` for
//! `examples/README.md` to catalogue.
//!
//! **Feature-*sighted* by construction, which is the whole point.**
//! [`fixture.rs`](fixture.rs) holds no `#[cfg]` so that the same program is
//! built at every point and a difference between two builds is the feature.
//! That answers what enabling `F` costs a program that does not use `F`, and it
//! is the only question one program can answer: the fixture never names a
//! codec, so the linker strips what nothing called and `json`, `form`,
//! `multipart`, `protobuf` and `compression` all read zero there.
//!
//! `docs/performance.md`'s shape table bills an opt-in payload codec "a binary
//! delta, and an allocation count on an operation that names it", and says
//! plainly that the delta is taken "on a route that mounts it". A route that
//! mounts a codec cannot exist in a program that also builds without the flag,
//! so the two questions cannot share a fixture. This is the second one.
//!
//! **What is constant across the points, and what is not.** Two operations are
//! mounted at every point and read the transport rather than a codec: one
//! `POST` taking [`Binary<OctetStream>`] and dropping it, one `GET` returning
//! the same. They are the floor
//! [`tests/alloc_codecs.rs`](../tests/alloc_codecs.rs) calls the *transport
//! floor*, and they are here for its reason — without them "reads a body at
//! all" would be charged to the first codec measured. Each codec then adds its
//! own pair on top, behind its own flag, so the baseline point mounts the floor
//! and nothing else and each codec point mounts the floor plus one codec.
//!
//! **A codec's delta is its code, its dependency's code, and its payload
//! type's.** Mounting `Json` pulls `serde_json`; `Form` pulls
//! `serde_urlencoded`; `MultipartForm` pulls `multer`; `Protobuf` pulls
//! `prost`; `Compression` pulls `async-compression` and its three encoders. A
//! payload type is not free either: it carries a `Schema` derive and the
//! codec's own serialization derive, and a mounted operation that named no type
//! would not be a mounted operation. So a row of `codec.tsv` is the cost of the
//! whole arrangement to the artifact, which is what a program mounting a codec
//! actually pays, and it is not attributable to Kynos alone. `codec.tsv` says
//! so per row.
//!
//! **It never serves**, for [`fixture.rs`](fixture.rs)'s reason: `server` is
//! excluded from the sweep because it does not compile without an HTTP
//! protocol, so a fixture that bound a socket could not be built at the point
//! the deltas are taken against.
//!
//! **`macros` is required where [`fixture.rs`](fixture.rs) requires nothing.**
//! An operation is declared with an attribute macro, so there is no mounting a
//! codec without it. That costs no coverage: every point this is measured at
//! carries `macros`, and those six points are exactly the six
//! `mise run lint:codecs` already lints every `kynos` target at, so this file
//! is Clippy-checked at each feature set it is measured at.
//!
//! [`scripts/cost_features.py`]: ../../../scripts/cost_features.py
//! [`Binary<OctetStream>`]: kynos::extract::body::binary::Binary

use kynos::{
    extract::{body::binary::Binary, media::OctetStream},
    openapi::Info,
    prelude::*,
};

/// The transport floor, inbound: the octets are read and dropped undecoded.
#[kynos::post("/floor/bytes")]
async fn floor_bytes(body: Binary<OctetStream>) -> NoContent {
    drop(body.into_inner());
    NoContent
}

/// The transport floor, outbound: octets written with no codec over them.
#[kynos::get("/floor/out")]
async fn floor_out() -> Binary<OctetStream> {
    Binary::new(bytes::Bytes::from_static(b"{}"))
}

#[cfg(feature = "json")]
mod json {
    //! One operation each way over [`Json`](kynos::extract::body::json::Json).

    use kynos::prelude::*;

    /// The payload both directions carry.
    ///
    /// All-integer and shared in shape with the `form` and `protobuf` modules
    /// below, so three codecs are measured over one value rather than three.
    #[derive(Schema, serde::Deserialize, serde::Serialize)]
    pub(crate) struct Reading {
        id: u64,
        value: u64,
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/json")]
    pub(crate) async fn decode(Json(reading): Json<Reading>) -> NoContent {
        let _ = reading;
        NoContent
    }

    /// The operation that names it on the way out.
    #[kynos::get("/json/out")]
    pub(crate) async fn encode() -> Json<Reading> {
        Json(Reading { id: 7, value: 11 })
    }
}

#[cfg(feature = "form")]
mod form {
    //! One operation each way over [`Form`](kynos::extract::body::form::Form).

    use kynos::{extract::body::form::Form, prelude::*};

    /// The payload both directions carry, in the `json` module's shape.
    #[derive(Schema, serde::Deserialize, serde::Serialize)]
    pub(crate) struct Reading {
        id: u64,
        value: u64,
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/form")]
    pub(crate) async fn decode(Form(reading): Form<Reading>) -> NoContent {
        let _ = reading;
        NoContent
    }

    /// The operation that names it on the way out.
    #[kynos::get("/form/out")]
    pub(crate) async fn encode() -> Form<Reading> {
        Form(Reading { id: 7, value: 11 })
    }
}

#[cfg(feature = "multipart")]
mod multipart {
    //! One operation each way over
    //! [`MultipartForm`](kynos::extract::body::multipart::MultipartForm).

    use kynos::{extract::body::multipart::MultipartForm, prelude::*};

    /// The payload both directions carry.
    ///
    /// One `String` field rather than the all-integer struct the other codecs
    /// carry, for the reason `tests/alloc_codecs.rs` gives: a multipart part is
    /// octets plus a media type, and the field shape that has a `Schema` is an
    /// owned one.
    #[derive(Schema, MultipartForm)]
    pub(crate) struct Upload {
        note: String,
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/multipart")]
    pub(crate) async fn decode(MultipartForm(upload): MultipartForm<Upload>) -> NoContent {
        drop(upload);
        NoContent
    }

    /// The operation that names it on the way out.
    #[kynos::get("/multipart/out")]
    pub(crate) async fn encode() -> MultipartForm<Upload> {
        MultipartForm(Upload {
            note: "seven".to_owned(),
        })
    }
}

#[cfg(feature = "protobuf")]
mod protobuf {
    //! One operation each way over
    //! [`Protobuf`](kynos::extract::body::protobuf::Protobuf).

    use kynos::{extract::body::protobuf::Protobuf, prelude::*};

    /// The payload both directions carry, in the `json` module's shape.
    ///
    /// Derived twice, for the reason `examples/protobuf.rs` gives at length:
    /// `prost::Message` decides the octets and `Schema` decides what the
    /// description says they mean, and neither is derivable from the other.
    #[derive(Clone, PartialEq, prost::Message, Schema)]
    pub(crate) struct Reading {
        #[prost(uint64, tag = "1")]
        id: u64,
        #[prost(uint64, tag = "2")]
        value: u64,
    }

    /// The operation that names the codec on the way in.
    #[kynos::post("/protobuf")]
    pub(crate) async fn decode(Protobuf(reading): Protobuf<Reading>) -> NoContent {
        let _ = reading;
        NoContent
    }

    /// The operation that names it on the way out.
    #[kynos::get("/protobuf/out")]
    pub(crate) async fn encode() -> Protobuf<Reading> {
        Protobuf(Reading { id: 7, value: 11 })
    }
}

fn main() {
    let router = Router::<()>::new()
        .info(Info::new("codec cost fixture", "0.0.0"))
        .mount(kynos::routes![floor_bytes, floor_out]);

    #[cfg(feature = "json")]
    let router = router.mount(kynos::routes![json::decode, json::encode]);
    #[cfg(feature = "form")]
    let router = router.mount(kynos::routes![form::decode, form::encode]);
    #[cfg(feature = "multipart")]
    let router = router.mount(kynos::routes![multipart::decode, multipart::encode]);
    #[cfg(feature = "protobuf")]
    let router = router.mount(kynos::routes![protobuf::decode, protobuf::encode]);
    // `compression` mounts no operation of its own: it is an interceptor over
    // the ones already here, which is what mounting it means for that feature.
    #[cfg(feature = "compression")]
    let router = router.intercept(kynos::middleware::compression::Compression::new());

    // `black_box` on the built service, not only on the document: the document
    // is produced by `describe` and a linker that saw only that could argue the
    // dispatch table is dead. Forcing the `Service` keeps the erased handlers --
    // and therefore the codec code they call -- in the artifact, which is the
    // thing being weighed.
    let service = std::hint::black_box(router.build(()).expect("the fixture describes"));
    let document = service.openapi().to_json().expect("the document emits");

    println!("{}", std::hint::black_box(document).len());
}
