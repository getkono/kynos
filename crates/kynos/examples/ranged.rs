//! Serving a large representation from somewhere that is not a disk.
//!
//! ```text
//! cargo run -p kynos --example ranged --no-default-features \
//!   --features openapi31,macros,server,http1,json
//! ```
//!
//! [`assets.rs`](assets.rs) serves files. This serves a representation the
//! process does not have and may never hold all of: a `ByteSource` is asked for
//! a span and returns that span, so a client resuming a download at the 900MB
//! mark costs one span rather than one file.
//!
//! Five things are worth noticing:
//!
//! * **The source is a trait, not a path.** The one here is a synthetic
//!   generator, which is the point — an object store, a decrypting reader or a
//!   fake in a test all satisfy the same two methods, and none of them is a
//!   file. `Rangeable` is the other half of the pair and stays sealed: it
//!   answers what a byte range *means* for a body already in hand, which is a
//!   closed question. Where octets come from is not.
//! * **Nothing is buffered.** `complete_length` is asked once, before a byte is
//!   read, so an unsatisfiable request costs no read at all — and a whole
//!   representation streams in spans rather than arriving in memory first.
//! * **The conditions are one argument.** `Conditions` carries `Range`,
//!   `If-Range`, `If-None-Match` and `If-Modified-Since` together, because the
//!   specification fixes the order they are evaluated in and a handler taking
//!   them separately could apply them in the wrong one. Taking it is also what
//!   puts all four in the emitted description.
//! * **A strong validator is what makes a resume safe.** `If-Range` is only
//!   defined against one, so a source that cannot mint a strong `ETag` is
//!   telling you it cannot support resumption. `Last-Modified` is the weaker
//!   partner and is ranked below it, per section 13.1.3.
//! * **HEAD is its own route.** Kynos does not invent one from a GET, because
//!   the two are separate operations in the description. Both reach the same
//!   builder, and `Served` sends no content for the second.

use std::{
    net::Ipv4Addr,
    time::{Duration, SystemTime},
};

use bytes::Bytes;
use kynos::{
    Router,
    extract::media::MediaType,
    http::etag::ETag,
    response::range::{
        served::{Conditions, Delivery, Served},
        source::ByteSource,
    },
    server::Server,
};

/// A recording, as far as a client is concerned.
struct Mpeg;

impl MediaType for Mpeg {
    const MEDIA_TYPE: &'static str = "audio/mpeg";
}

/// Half a gigabyte that exists nowhere.
///
/// Synthetic on purpose: a source this large would be absurd to hold, which is
/// the property the whole trait exists to preserve. Swap it for an object
/// store and nothing above it changes.
struct Synthetic {
    length: u64,
}

impl ByteSource for Synthetic {
    // A generator cannot fail. A real store's error type is where a missing
    // object, a permission failure and a timeout are told apart -- Kynos hands
    // it back rather than deciding which status any of them deserves.
    type Error = std::convert::Infallible;

    async fn complete_length(&self) -> Result<u64, Self::Error> {
        Ok(self.length)
    }

    async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
        // Deterministic from the offset, so a resumed download splices into
        // exactly what a whole one would have produced -- which is what makes
        // the `If-Range` guarantee observable by hand.
        let span = usize::try_from(last - first + 1).unwrap_or(0);
        let mut octets = Vec::with_capacity(span);
        for offset in first..=last {
            octets.push(b'a' + u8::try_from(offset % 26).unwrap_or(0));
        }
        Ok(Bytes::from(octets))
    }
}

/// The recording, ready to deliver.
fn recording() -> Served<Synthetic, Mpeg> {
    Served::new(Synthetic {
        length: 512 * 1024 * 1024,
    })
    // Strong, because `If-Range` is defined against nothing else. A generator
    // whose output depends only on the offset really is byte-for-byte stable,
    // so the claim is honest here; a store that cannot make it should send a
    // weak tag and accept that resumption is unavailable.
    .etag(ETag::strong("synthetic-v1"))
    .last_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
    .cache_control("public, max-age=86400")
    .attachment("recording.mp3")
}

/// Streams the recording, honouring a range and the conditions around it.
#[kynos::get("/recordings/current")]
async fn get_recording(conditions: Conditions) -> Delivery<Mpeg> {
    recording()
        .deliver(&conditions)
        .await
        .expect("a generator cannot fail")
}

/// The same fields with no content, for a client sizing the download first.
#[kynos::head("/recordings/current")]
async fn head_recording(conditions: Conditions) -> Delivery<Mpeg> {
    recording()
        .deliver(&conditions)
        .await
        .expect("a generator cannot fail")
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![get_recording, head_recording]);

    println!("{}", router.openapi()?.to_json()?);

    // Try it:
    //   curl -sD- -o/dev/null http://localhost:3000/recordings/current
    //   curl -sD- -H 'Range: bytes=0-15' http://localhost:3000/recordings/current
    //   curl -sD- -H 'If-None-Match: "synthetic-v1"' http://localhost:3000/recordings/current
    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
