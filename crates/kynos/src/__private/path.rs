//! Const-evaluable comparisons, for the assertions a route attribute emits.
//!
//! A const panic formats nothing but a single `&str` on the declared MSRV, so
//! the attribute cannot render a name it reads here into its message. It
//! asks per position instead, and every answer it can render is one whose
//! names it already holds as literals: the route's own variables.

/// How the parameter a `PathParams` group declares at one position compares
/// with the route variable at that position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathParameter {
    /// Named exactly as the route variable at that position.
    Matches,
    /// The group declares fewer parameters than the route has variables.
    Missing,
    /// Named as the route variable at the carried position instead.
    Moved(usize),
    /// Named as none of the route's variables.
    Unknown,
}

/// Compares the parameter `names` declares at `index` with the route
/// `variables`, in const code.
///
/// `index` is a position among `variables`; a group longer than the route is
/// the caller's to detect, by comparing lengths.
#[must_use]
pub const fn path_parameter_at(names: &[&str], variables: &[&str], index: usize) -> PathParameter {
    if index >= names.len() {
        return PathParameter::Missing;
    }
    let name = names[index];
    if index < variables.len() && const_str_eq(name, variables[index]) {
        return PathParameter::Matches;
    }
    let mut position = 0;
    while position < variables.len() {
        if const_str_eq(name, variables[position]) {
            return PathParameter::Moved(position);
        }
        position += 1;
    }
    PathParameter::Unknown
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
