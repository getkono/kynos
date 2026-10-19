use super::{HeaderMap, HeaderValue, header, header_name_eq, vary_on};

#[test]
fn header_names_compare_without_regard_to_case() {
    assert!(header_name_eq("X-Request-Id", "x-request-id"));
    assert!(!header_name_eq("x-request-id", "x-request-ids"));
    assert!(!header_name_eq("x-a", "x-b"));
}

/// Every `Vary` line a response carries after `names` merge into `lines`.
fn vary_lines_after(lines: &[&[u8]], names: &'static [&'static str]) -> Vec<Vec<u8>> {
    let mut fields = HeaderMap::new();

    for line in lines {
        fields.append(
            header::VARY,
            HeaderValue::from_bytes(line).expect("a representable Vary line"),
        );
    }

    vary_on(&mut fields, names);

    fields
        .get_all(header::VARY)
        .iter()
        .map(|value| value.as_bytes().to_owned())
        .collect()
}

/// RFC 9110 section 5.3 lets a list field arrive split across lines, so a
/// name on the second line is as much a member as one on the first.
/// Reading only the first and then replacing every line dropped `cookie`
/// here, and a cache keyed on what was left served one user's response to
/// another.
#[test]
fn a_vary_union_keeps_the_field_names_of_every_line() {
    let lines = vary_lines_after(&[b"accept-language", b"cookie"], &["accept-encoding"]);

    assert_eq!(
        lines,
        [b"accept-language, cookie, accept-encoding".to_vec()]
    );
}

/// A wildcard on a later line absorbs every name just as one on the first
/// does.
#[test]
fn a_wildcard_on_any_vary_line_absorbs_every_name_added_to_it() {
    let lines = vary_lines_after(&[b"accept", b"*"], &["origin"]);

    assert_eq!(lines, [b"accept".to_vec(), b"*".to_vec()]);
}

/// Lines that already hold every name stay as they were: the merge rewrites
/// the field only when it has a name to add.
#[test]
fn vary_lines_that_already_hold_every_name_are_left_as_they_were() {
    let lines = vary_lines_after(&[b"origin", b"cookie"], &["origin"]);

    assert_eq!(lines, [b"origin".to_vec(), b"cookie".to_vec()]);
}

/// A line that is not UTF-8 still names something a cache must key on, so
/// it survives byte for byte, and so does every line beside it.
#[test]
fn a_vary_line_that_is_not_utf8_is_kept_with_every_other_line() {
    let lines = vary_lines_after(&[b"x-caf\xe9", b"cookie"], &["origin"]);

    assert_eq!(lines, [b"x-caf\xe9, cookie, origin".to_vec()]);
}

/// Names the field lacks are added once each, in their order, however many
/// spellings of one name the caller passes.
#[test]
fn names_added_to_vary_are_joined_once_each_in_order() {
    let lines = vary_lines_after(&[b"ORIGIN"], &["origin", "accept", "Accept", "cookie"]);

    assert_eq!(lines, [b"ORIGIN, accept, cookie".to_vec()]);
}

/// The one name added to a field holding no member is the whole line, and
/// a line of only separators holds no member to keep.
#[test]
fn a_lone_name_added_to_an_empty_vary_is_the_whole_line() {
    assert_eq!(vary_lines_after(&[], &["origin"]), [b"origin".to_vec()]);
    assert_eq!(
        vary_lines_after(&[b" , "], &["origin"]),
        [b"origin".to_vec()]
    );
}
