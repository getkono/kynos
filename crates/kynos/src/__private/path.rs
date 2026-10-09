//! Const-evaluable comparisons, for the assertions a route attribute emits.
//!
//! A const panic renders a single `&str` and formats nothing into it, so the
//! message naming both sides of a mismatch is composed here, in a fixed
//! buffer, and panicked with whole.

/// The rule every mismatch message ends with.
const RULE: &str = "PathParams names must match the route's variables one for one, in order";

/// Describes the first disagreement between the parameter `names` a
/// `PathParams` group declares and its route's `variables`, in const code.
///
/// `group` and `path` are only rendered: the group's type and the route
/// template, as the attribute read them. `None` when the two lists are equal.
#[must_use]
pub const fn path_parameter_mismatch(
    group: &str,
    path: &str,
    names: &[&str],
    variables: &[&str],
) -> Option<Message> {
    let mut message = Message::new();
    message.push("`");
    message.push(group);
    let mut index = 0;
    while index < names.len() && index < variables.len() {
        if !const_str_eq(names[index], variables[index]) {
            message.push("` declares path parameter `");
            message.push(names[index]);
            message.push("` where the route `");
            message.push(path);
            message.push("` has variable `");
            message.push(variables[index]);
            message.push("`; ");
            message.push(RULE);
            return Some(message);
        }
        index += 1;
    }
    if index < variables.len() {
        message.push("` declares no path parameter for variable `");
        message.push(variables[index]);
        message.push("` of the route `");
        message.push(path);
        message.push("`; ");
        message.push(RULE);
        return Some(message);
    }
    if index < names.len() {
        message.push("` declares path parameter `");
        message.push(names[index]);
        message.push("`, for which the route `");
        message.push(path);
        message.push("` has no variable; ");
        message.push(RULE);
        return Some(message);
    }
    None
}

/// A message composed in const code, truncated at a character boundary if it
/// outgrows its buffer.
#[derive(Clone, Copy, Debug)]
pub struct Message {
    bytes: [u8; CAPACITY],
    len: usize,
}

/// Long enough for any name a path template holds; a longer message is cut
/// short rather than refused, since it is already an error.
const CAPACITY: usize = 1024;

impl Message {
    const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            len: 0,
        }
    }

    const fn push(&mut self, text: &str) {
        let text = text.as_bytes();
        let mut end = text.len();
        if end > CAPACITY - self.len {
            end = CAPACITY - self.len;
            // Back off any continuation byte, so the cut falls between
            // characters and the buffer stays valid UTF-8. `end` is below
            // `text.len()` here, and `text[0]` starts a character, so the
            // loop stays in bounds and stops by index 0.
            while text[end] & 0b1100_0000 == 0b1000_0000 {
                end -= 1;
            }
        }
        let mut index = 0;
        while index < end {
            self.bytes[self.len + index] = text[index];
            index += 1;
        }
        self.len += end;
    }

    /// The message composed so far.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        match core::str::from_utf8(self.bytes.split_at(self.len).0) {
            Ok(text) => text,
            // Unreachable: only whole `&str`s, or prefixes of one cut between
            // characters, are ever pushed.
            Err(_) => RULE,
        }
    }
}

const fn const_str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}
