//! JSON Pointers into a document, as every location `emit` and `validate`
//! report is written: RFC 6901 behind a `#`.

use std::fmt::Write as _;

/// Escapes one map key for use as a JSON Pointer token, per RFC 6901.
///
/// Every `paths` key contains a `/`, which must not split the location.
pub(crate) fn pointer_token(key: &str) -> String {
    let mut token = String::with_capacity(escaped_len(key));
    push_escaped(&mut token, key);
    token
}

/// The length of `key` once escaped.
fn escaped_len(key: &str) -> usize {
    key.len()
        + key
            .bytes()
            .filter(|byte| matches!(byte, b'~' | b'/'))
            .count()
}

/// Appends `key` to `pointer`, escaped.
fn push_escaped(pointer: &mut String, key: &str) {
    for character in key.chars() {
        match character {
            '~' => pointer.push_str("~0"),
            '/' => pointer.push_str("~1"),
            character => pointer.push(character),
        }
    }
}

/// A location in a document, as a chain of borrowed tokens on the stack.
///
/// Rendered only by [`pointer`](Self::pointer), so a walk that reports nothing
/// costs no allocation (held by `tests/alloc.rs`).
#[derive(Clone, Copy)]
pub(crate) struct At<'a> {
    parent: Option<&'a At<'a>>,
    token: Token<'a>,
}

#[derive(Clone, Copy)]
enum Token<'a> {
    Root,
    Key(&'a str),
    Index(usize),
}

impl<'a> At<'a> {
    /// The document itself, `#`.
    pub(crate) const ROOT: At<'static> = At {
        parent: None,
        token: Token::Root,
    };

    /// The member `key` of the object here.
    pub(crate) fn key(&'a self, key: &'a str) -> Self {
        Self {
            parent: Some(self),
            token: Token::Key(key),
        }
    }

    /// The element `index` of the array here.
    pub(crate) fn index(&'a self, index: usize) -> Self {
        Self {
            parent: Some(self),
            token: Token::Index(index),
        }
    }

    /// The pointer, escaped, in one allocation.
    pub(crate) fn pointer(&self) -> String {
        let mut pointer = String::with_capacity(self.len());
        self.render(&mut pointer);
        pointer
    }

    /// How long [`pointer`](Self::pointer) is.
    fn len(&self) -> usize {
        let own = match self.token {
            Token::Root => 1,
            Token::Key(key) => 1 + escaped_len(key),
            Token::Index(index) => 1 + digits(index),
        };
        own + self.parent.map_or(0, At::len)
    }

    fn render(&self, pointer: &mut String) {
        if let Some(parent) = self.parent {
            parent.render(pointer);
        }
        match self.token {
            Token::Root => pointer.push('#'),
            Token::Key(key) => {
                pointer.push('/');
                push_escaped(pointer, key);
            }
            Token::Index(index) => {
                // Writing to a `String` cannot fail.
                let _ = write!(pointer, "/{index}");
            }
        }
    }
}

/// How many decimal digits `index` is written in.
fn digits(mut index: usize) -> usize {
    let mut digits = 1;
    while index >= 10 {
        index /= 10;
        digits += 1;
    }
    digits
}

#[cfg(test)]
mod tests {
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
}
