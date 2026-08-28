//! Ranged delivery over a source that is not a filesystem.
//!
//! One reason: the whole point of `ByteSource` is that the octets need not be
//! on disk, and nothing else in the suite drives a representation that is not.
//! The fake here is the shape an application's test double takes — an object
//! store, a decrypting reader, a fixture in memory — and it records the widest
//! span it was asked for, because *what is read* is the property the
//! abstraction exists to guarantee.

#![cfg(all(feature = "macros", feature = "json"))]

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

use bytes::Bytes;
use kynos::{
    Router,
    extract::media::MediaType,
    http::etag::ETag,
    http::{Method, StatusCode},
    response::range::{
        served::{Conditions, Delivery, Served},
        source::ByteSource,
    },
};

/// The media type the recording is sent as, at the type level so the emitted
/// description can name it without a value to look at.
struct Mpeg;

impl MediaType for Mpeg {
    const MEDIA_TYPE: &'static str = "audio/mpeg";
}

#[path = "support/mod.rs"]
mod support;

use support::{get, send};

/// The representation every test here serves.
const RECORDING: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// 1994-11-06 08:49:37 GMT, which is section 5.6.7's own example date.
const MODIFIED: u64 = 784_111_777;
const MODIFIED_AS_SENT: &str = "Sun, 06 Nov 1994 08:49:37 GMT";

/// A source with no filesystem under it.
#[derive(Clone)]
struct Catalogue {
    octets: Bytes,
    widest: Arc<AtomicU64>,
}

impl Catalogue {
    fn new() -> Self {
        Self {
            octets: Bytes::from_static(RECORDING),
            widest: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ByteSource for Catalogue {
    type Error = std::convert::Infallible;

    async fn complete_length(&self) -> Result<u64, Self::Error> {
        Ok(self.octets.len() as u64)
    }

    async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
        self.widest.fetch_max(last - first + 1, Ordering::Relaxed);
        let first = usize::try_from(first).unwrap_or(usize::MAX);
        let end = usize::try_from(last + 1)
            .unwrap_or(usize::MAX)
            .min(self.octets.len());
        Ok(self.octets.slice(first..end))
    }
}

/// Serves the recording, conditionally and rangeably.
#[kynos::get("/recordings/current")]
async fn recording(conditions: Conditions) -> Delivery<Mpeg> {
    Served::<_, Mpeg>::new(Catalogue::new())
        .etag(ETag::strong("r3"))
        .last_modified(UNIX_EPOCH + Duration::from_secs(MODIFIED))
        .cache_control("public, max-age=3600")
        .attachment("recording.mp3")
        .deliver(&conditions)
        .await
        .expect("this source cannot fail")
}

/// The same representation, answered to a HEAD.
///
/// A separate route because Kynos does not derive HEAD from GET: the two are
/// distinct operations in the description, and a router that invented one would
/// be describing an endpoint nobody declared. `Served` answers both -- section
/// 9.3.2 says a HEAD is "identical to GET except that the server MUST NOT send
/// content", and the fields below are the same ones the GET sends.
#[kynos::head("/recordings/current")]
async fn recording_head(conditions: Conditions) -> Delivery<Mpeg> {
    Served::<_, Mpeg>::new(Catalogue::new())
        .etag(ETag::strong("r3"))
        .last_modified(UNIX_EPOCH + Duration::from_secs(MODIFIED))
        .cache_control("public, max-age=3600")
        .attachment("recording.mp3")
        .deliver(&conditions)
        .await
        .expect("this source cannot fail")
}

fn service() -> kynos::router::service::Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![recording, recording_head])
        .build(())
        .expect("a describable router")
}

// --- The statuses the contract names --------------------------------------

/// 200, with everything a representation owes.
#[tokio::test]
async fn the_whole_representation_carries_what_it_owes() {
    let reply = get(&service(), "/recordings/current").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, RECORDING);
    assert_eq!(reply.field("accept-ranges").as_deref(), Some("bytes"));
    assert_eq!(reply.field("content-type").as_deref(), Some("audio/mpeg"));
    assert_eq!(reply.field("etag").as_deref(), Some("\"r3\""));
    assert_eq!(
        reply.field("last-modified").as_deref(),
        Some(MODIFIED_AS_SENT)
    );
    assert_eq!(
        reply.field("content-length").as_deref(),
        Some(RECORDING.len().to_string().as_str())
    );
    assert_eq!(
        reply.field("cache-control").as_deref(),
        Some("public, max-age=3600")
    );
    assert_eq!(
        reply.field("content-disposition").as_deref(),
        Some("attachment; filename=\"recording.mp3\"")
    );
}

/// 206, with the part and the field naming it.
#[tokio::test]
async fn a_range_is_answered_with_the_part_it_named() {
    let reply = get(&service(), "/recordings/current")
        .header("range", "bytes=10-19")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(reply.body, &RECORDING[10..=19]);
    assert_eq!(
        reply.field("content-range").as_deref(),
        Some("bytes 10-19/36")
    );
    assert_eq!(reply.field("content-length").as_deref(), Some("10"));
    // Section 15.3.7 asks a 206 for the fields a 200 would have carried.
    assert_eq!(reply.field("etag").as_deref(), Some("\"r3\""));
    assert_eq!(reply.field("content-type").as_deref(), Some("audio/mpeg"));
}

/// A suffix range counts back from the end.
#[tokio::test]
async fn a_suffix_range_counts_from_the_end() {
    let reply = get(&service(), "/recordings/current")
        .header("range", "bytes=-6")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(reply.body, &RECORDING[30..]);
    assert_eq!(
        reply.field("content-range").as_deref(),
        Some("bytes 30-35/36")
    );
}

/// 416, stating the complete length rather than a part.
#[tokio::test]
async fn a_range_past_the_end_is_not_satisfiable() {
    let reply = get(&service(), "/recordings/current")
        .header("range", "bytes=900-999")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(reply.field("content-range").as_deref(), Some("bytes */36"));
}

/// An unusable `Range` is ignored rather than refused.
///
/// Section 14.2 lists the reasons, and every one of them answers with the whole
/// representation. A 400 for any of them would refuse a request the
/// specification says to serve.
#[tokio::test]
async fn an_unusable_range_field_is_ignored() {
    for value in ["items=0-1", "bytes=", "nonsense"] {
        let reply = get(&service(), "/recordings/current")
            .header("range", value)
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK, "{value:?}");
        assert_eq!(reply.body, RECORDING, "{value:?}");
    }
}

// --- The conditions -------------------------------------------------------

/// 304 from a matching entity tag.
#[tokio::test]
async fn a_current_copy_is_answered_with_304() {
    let reply = get(&service(), "/recordings/current")
        .header("if-none-match", "\"r3\"")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_MODIFIED);
    assert!(reply.body.is_empty());
    assert_eq!(reply.field("etag").as_deref(), Some("\"r3\""));
    // Section 14.3: a 304 carries no representation, so it advertises no range
    // of one.
    assert_eq!(reply.field("accept-ranges"), None);
}

/// 304 from a date, where there is no tag to prefer.
#[tokio::test]
async fn a_date_that_is_current_is_answered_with_304() {
    let reply = get(&service(), "/recordings/current")
        .header("if-modified-since", MODIFIED_AS_SENT)
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_MODIFIED);

    // A date before the last change is not current.
    let older = get(&service(), "/recordings/current")
        .header("if-modified-since", "Sat, 05 Nov 1994 08:49:37 GMT")
        .call()
        .await;
    assert_eq!(older.status, StatusCode::OK);
}

/// The tag outranks the date, which section 13.1.3 requires in as many words.
///
/// A request carrying both a stale tag and a current date is served, not
/// answered 304 — "a recipient MUST ignore If-Modified-Since if the request
/// contains an If-None-Match header field".
#[tokio::test]
async fn a_tag_is_preferred_over_a_date() {
    let reply = get(&service(), "/recordings/current")
        .header("if-none-match", "\"stale\"")
        .header("if-modified-since", MODIFIED_AS_SENT)
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, RECORDING);
}

/// A malformed date is no condition at all.
#[tokio::test]
async fn a_malformed_date_is_ignored() {
    let reply = get(&service(), "/recordings/current")
        .header("if-modified-since", "yesterday")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
}

/// 304 wins over 206, because section 14.2 evaluates the range last.
///
/// The ordering that is easy to get backwards: a `Range` is applied "only if
/// the result in absence of the Range header field would be a 200".
#[tokio::test]
async fn a_condition_that_matches_beats_a_range() {
    let reply = get(&service(), "/recordings/current")
        .header("if-none-match", "\"r3\"")
        .header("range", "bytes=0-3")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_MODIFIED);
}

/// `If-Range` resumes against the tag that named the representation.
#[tokio::test]
async fn if_range_admits_a_resume_and_refuses_a_stale_one() {
    let resumed = get(&service(), "/recordings/current")
        .header("if-range", "\"r3\"")
        .header("range", "bytes=10-19")
        .call()
        .await;
    assert_eq!(resumed.status, StatusCode::PARTIAL_CONTENT);

    // A tag naming something else gets the whole representation rather than a
    // part the client would splice onto the wrong prefix.
    let stale = get(&service(), "/recordings/current")
        .header("if-range", "\"r2\"")
        .header("range", "bytes=10-19")
        .call()
        .await;
    assert_eq!(stale.status, StatusCode::OK);
    assert_eq!(stale.body, RECORDING);
}

// --- HEAD -----------------------------------------------------------------

/// HEAD answers everything GET would, and sends no content.
///
/// Section 9.3.2, and the reason it matters here: a client discovers a
/// representation's length and resumability with a HEAD before committing to
/// the download.
#[tokio::test]
async fn head_carries_every_field_and_no_body() {
    let reply = send(&service(), Method::HEAD, "/recordings/current")
        .call()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.is_empty());
    assert_eq!(reply.field("accept-ranges").as_deref(), Some("bytes"));
    assert_eq!(
        reply.field("content-length").as_deref(),
        Some(RECORDING.len().to_string().as_str())
    );
    assert_eq!(reply.field("etag").as_deref(), Some("\"r3\""));
}

// --- The description ------------------------------------------------------

/// Every field the delivery reads is declared, and every status it can send.
#[test]
fn the_description_names_what_a_delivery_reads_and_sends() {
    let document = Router::<()>::new()
        .mount(kynos::routes![recording])
        .openapi()
        .expect("a describable router");

    let operation = document.paths.items["/recordings/current"]
        .get
        .as_ref()
        .expect("a GET");

    let mut declared: Vec<&str> = operation
        .parameters
        .iter()
        .filter_map(|parameter| parameter.as_item())
        .map(|parameter| parameter.name.as_str())
        .collect();
    declared.sort_unstable();

    assert_eq!(
        declared,
        ["If-Modified-Since", "If-None-Match", "If-Range", "Range"]
    );
}
