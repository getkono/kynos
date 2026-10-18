//! The `Content-Disposition` reader every multipart part goes through, and the
//! quoting the multipart response writer applies to what it reads.
//!
//! The input is one field value. Whatever the reader admits, the writer's
//! quoting of its `name` and `filename` reads back to the same values, for
//! values holding none of the `\`, CR and LF the writer drops: a value the
//! reader yields is one the writer can carry, and the two agree on escaping.

#![no_main]

use kynos::__private::fuzz;
use libfuzzer_sys::fuzz_target;

/// The characters the writer drops rather than quotes.
const DROPPED: [char; 3] = ['\\', '\r', '\n'];

fuzz_target!(|data: &[u8]| {
    let Ok(field) = std::str::from_utf8(data) else {
        return;
    };
    let Ok((name, file_name)) = fuzz::disposition(field) else {
        return;
    };
    if name.contains(DROPPED) || file_name.as_ref().is_some_and(|it| it.contains(DROPPED)) {
        return;
    }

    let mut written = format!("form-data; name=\"{}\"", fuzz::quoted(&name));
    if let Some(file_name) = &file_name {
        written.push_str(&format!("; filename=\"{}\"", fuzz::quoted(file_name)));
    }
    assert_eq!(
        fuzz::disposition(&written),
        Ok((name, file_name)),
        "{field:?} rewritten as {written:?}"
    );
});
