//! A variant that does not say what status it produces would leave the
//! description guessing, so the derive refuses rather than choosing one.

use kynos::Reply;

#[derive(Reply)]
enum CreateReply {
    Created(u32),
}

fn main() {}
