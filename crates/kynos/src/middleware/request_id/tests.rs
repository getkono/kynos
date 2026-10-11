use super::{Random, RequestIdSource};

/// An identifier is 128 bits written as 32 lowercase hex digits, leading
/// zeros included, so every identifier has the same length.
#[test]
fn a_random_identifier_is_thirty_two_lowercase_hex_digits() {
    let source = Random::new();

    for _ in 0..64 {
        let id = source.next_id();
        let id = id.to_str().expect("hex digits are visible ASCII");

        assert_eq!(id.len(), 32, "{id}");
        assert!(
            id.bytes()
                .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f')),
            "{id}"
        );
    }
}

/// The counter behind the hash never reaches the wire: successive identifiers
/// differ, and none is the counter's own value.
#[test]
fn successive_random_identifiers_differ() {
    let source = Random::new();
    let ids: std::collections::HashSet<String> = (0..1024)
        .map(|_| source.next_id().to_str().expect("hex digits").to_owned())
        .collect();

    assert_eq!(ids.len(), 1024);
    for n in 0..1024_u32 {
        assert!(!ids.contains(format!("{n:032x}").as_str()), "{n} leaked");
    }
}

/// Two sources stand for two processes: the same count maps to different
/// identifiers, which is what makes an identifier unique across a restart.
#[test]
fn two_random_sources_map_one_count_apart() {
    let (first, second) = (Random::new(), Random::new());

    assert_ne!(first.halves(0), second.halves(0));
    assert_ne!(first.next_id(), second.next_id());
}

/// The two halves are hashed apart, so neither repeats the other.
#[test]
fn a_random_identifier_does_not_repeat_its_half() {
    let source = Random::new();
    let [high, low] = source.halves(7);

    assert_ne!(high, low);
}
