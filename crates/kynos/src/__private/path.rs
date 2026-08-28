//! Const-evaluable comparisons, for the assertions a route attribute emits.

/// Compares derived path parameter names with a route template in const code.
#[must_use]
pub const fn path_parameter_names_match(left: &[&str], right: &[&str]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if !const_str_eq(left[index], right[index]) {
            return false;
        }
        index += 1;
    }
    true
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
