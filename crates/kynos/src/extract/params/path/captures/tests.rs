use super::{INLINE, PathCaptures, Spans, narrow};

/// What a match on `path` records for `names`, each value `captured` gives
/// re-borrowed out of `path` itself, which is what the router yields and what
/// `PathCaptures::new` asserts.
pub(in crate::extract::params::path) fn recorded(
    path: &'static str,
    names: &'static [&'static str],
    captured: impl Fn(&str) -> Option<&'static str>,
) -> PathCaptures {
    PathCaptures::new(path, names, |name| {
        let value = captured(name)?;
        let start = path.find(value).expect("a capture inside the path");
        Some(&path[start..start + value.len()])
    })
}

/// The segment after `/v/` that `name`, a single lowercase letter, stands at:
/// `a` the first.
fn segment(path: &'static str, name: &str) -> Option<&'static str> {
    let index = usize::from(name.as_bytes()[0] - b'a');
    path.split('/').nth(index + 2)
}

/// Past [`INLINE`] variables, the record holds every span on the heap, and
/// each is still read back by its own name.
#[test]
fn captures_past_the_inline_count_keep_every_value() {
    let path = "/v/1/2/3/4/5";
    let recorded = recorded(path, &["a", "b", "c", "d", "e"], |name| segment(path, name));

    assert!(matches!(recorded.spans, Spans::Long(_)), "{recorded:?}");
    for (name, value) in [("a", "1"), ("b", "2"), ("c", "3"), ("d", "4"), ("e", "5")] {
        assert_eq!(recorded.get(path, name), Some(value), "{name}");
    }
}

/// Up to [`INLINE`] variables, the record holds its spans in place.
#[test]
fn captures_within_the_inline_count_stay_in_place() {
    const NAMES: [&str; INLINE] = ["a", "b", "c", "d"];
    let path = "/v/1/2/3/4";
    let recorded = recorded(path, &NAMES, |name| segment(path, name));

    assert!(matches!(recorded.spans, Spans::Short(_)), "{recorded:?}");
    for (name, value) in NAMES.into_iter().zip(["1", "2", "3", "4"]) {
        assert_eq!(recorded.get(path, name), Some(value), "{name}");
    }
}

/// A path whose offsets do not fit sixteen bits takes the wide spans rather
/// than truncating one, so a value at its end is still read back whole.
#[test]
fn a_path_past_sixteen_bit_offsets_is_read_back_whole() {
    let path: &'static str = format!("/v/{}/tail", "x".repeat(usize::from(u16::MAX))).leak();
    let recorded = recorded(path, &["tail"], |_| Some("tail"));

    assert!(matches!(recorded.spans, Spans::Long(_)), "{recorded:?}");
    assert_eq!(recorded.get(path, "tail"), Some("tail"));
}

/// A span ending past `u32::MAX` does not narrow to the wide width, and the
/// record it falls back to reads as absent rather than as a truncated value.
#[test]
fn a_span_past_thirty_two_bit_offsets_reads_as_absent() {
    let past = u64::from(u32::MAX) + 1;
    assert_eq!(narrow::<u64, u32>([0, past]), None);
    assert_eq!(narrow::<u64, u32>([past, past]), None);
    assert_eq!(
        narrow::<u64, u32>([0, u64::from(u32::MAX)]),
        Some([0, u32::MAX])
    );

    let path = "/v/1";
    let fallback = PathCaptures {
        names: &["a"],
        spans: Spans::Long(Box::new([[u32::MAX, 0]])),
    };
    assert_eq!(fallback.get(path, "a"), None);
}

/// A variable the template declares but the match did not capture reads as
/// absent, in either width, without disturbing its neighbours.
#[test]
fn an_uncaptured_variable_reads_as_absent() {
    let path = "/v/1/2";
    let short = recorded(path, &["a", "b"], |name| {
        (name == "b").then(|| segment(path, name)).flatten()
    });
    assert_eq!(short.get(path, "a"), None);
    assert_eq!(short.get(path, "b"), Some("2"));

    let long = recorded(path, &["a", "b", "c", "d", "e"], |name| {
        (name == "b").then(|| segment(path, name)).flatten()
    });
    assert!(matches!(long.spans, Spans::Long(_)), "{long:?}");
    assert_eq!(long.get(path, "a"), None);
    assert_eq!(long.get(path, "b"), Some("2"));
}
