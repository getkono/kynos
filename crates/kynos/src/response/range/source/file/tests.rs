use std::path::PathBuf;

use super::{ByteSource, FileSource, SPAN};

/// A scratch file of this test's own, removed when it drops.
///
/// Named by process and test, so no two tests ever share one.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str, contents: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!("kynos-source-{}-{test}", std::process::id()));
        std::fs::write(&path, contents).expect("a scratch file");
        Self(path)
    }

    async fn source(&self) -> FileSource {
        FileSource::new(std::fs::File::open(&self.0).expect("the scratch file"))
            .await
            .expect("a regular file")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// A span is the octets at its offsets, inclusive at both ends, and the length
/// is the one the metadata holds.
#[tokio::test]
async fn a_span_is_the_octets_at_its_offsets() {
    let scratch = Scratch::new("a_span_is_the_octets_at_its_offsets", ALPHABET);
    let source = scratch.source().await;

    assert_eq!(source.complete_length().await.unwrap(), 36);
    assert_eq!(source.metadata().len(), 36);
    assert_eq!(&source.read_span(10, 15).await.unwrap()[..], b"abcdef");
    assert_eq!(&source.read_span(35, 35).await.unwrap()[..], b"z");
}

/// Reads name their own offsets, so spans in flight together, in any order,
/// each get their own octets; a read through a shared cursor would hand one
/// the other's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_spans_do_not_share_a_cursor() {
    let contents: Vec<u8> = (0..4 * SPAN)
        .map(|offset| u8::try_from(offset % 251).unwrap())
        .collect();
    let scratch = Scratch::new("concurrent_spans_do_not_share_a_cursor", &contents);
    let source = std::sync::Arc::new(scratch.source().await);

    let spans = [3 * SPAN, 0, 2 * SPAN, SPAN].map(|first| {
        let source = std::sync::Arc::clone(&source);
        tokio::spawn(async move { (first, source.read_span(first, first + SPAN - 1).await) })
    });

    for span in spans {
        let (first, read) = span.await.unwrap();
        let start = usize::try_from(first).unwrap();
        assert_eq!(
            &read.unwrap()[..],
            &contents[start..start + usize::try_from(SPAN).unwrap()],
            "the span from {first}"
        );
    }
}

/// A span wider than [`SPAN`] is answered short, so a caller reading the
/// whole file in one call is not handed all of it in memory.
#[tokio::test]
async fn a_wide_span_is_answered_with_one_span() {
    let contents = vec![b'x'; usize::try_from(2 * SPAN).unwrap()];
    let scratch = Scratch::new("a_wide_span_is_answered_with_one_span", &contents);
    let source = scratch.source().await;

    let read = source.read_span(0, 2 * SPAN - 1).await.unwrap();

    assert_eq!(read.len(), usize::try_from(SPAN).unwrap());
}

/// The length is taken once: a file that shrinks afterwards answers the lost
/// span with no octets, which the body turns into `Truncated`, and one that
/// grows keeps the length its validators were minted from.
#[tokio::test]
async fn the_length_is_the_one_taken_when_the_source_was_built() {
    let scratch = Scratch::new(
        "the_length_is_the_one_taken_when_the_source_was_built",
        ALPHABET,
    );
    let source = scratch.source().await;

    std::fs::write(&scratch.0, &ALPHABET[..10]).unwrap();
    assert_eq!(source.complete_length().await.unwrap(), 36);
    assert!(source.read_span(20, 35).await.unwrap().is_empty());

    std::fs::write(&scratch.0, [ALPHABET, ALPHABET].concat()).unwrap();
    assert_eq!(source.complete_length().await.unwrap(), 36);
}

/// A directory opens as a handle on Unix but has no length to range over, so
/// it is refused when the source is built rather than on the first read.
#[cfg(unix)]
#[tokio::test]
async fn a_handle_that_is_not_a_regular_file_is_refused() {
    let directory = std::fs::File::open(std::env::temp_dir()).unwrap();

    let refused = FileSource::new(directory).await.unwrap_err();

    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
}
