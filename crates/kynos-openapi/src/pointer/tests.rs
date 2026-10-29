use super::{At, digits, pointer_token};

#[test]
fn a_token_escapes_tilde_before_slash() {
    assert_eq!(pointer_token("/a~1b/~"), "~1a~01b~1~0");
}

#[test]
fn a_pointer_is_rendered_escaped_and_exactly_sized() {
    let root = At::ROOT;
    let paths = root.key("paths");
    let item = paths.key("/pets/{id}");
    let parameters = item.key("parameters");
    let at = parameters.index(12);

    let pointer = at.pointer();

    assert_eq!(pointer, "#/paths/~1pets~1{id}/parameters/12");
    assert_eq!(pointer.capacity(), pointer.len());
}

#[test]
fn digits_counts_each_decade() {
    for (index, expected) in [(0, 1), (9, 1), (10, 2), (99, 2), (100, 3), (1_000_000, 7)] {
        assert_eq!(digits(index), expected, "{index}");
    }
}
