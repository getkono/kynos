//! A variant is one response, which has one status, so a second `status` on
//! it is refused rather than left to replace the first.
//!
//! The control is `pass/reply_with_status.rs`: one status on the variant.

use kynos::Reply;

#[derive(Reply)]
enum CreateReply {
    #[reply(status = 200, status = 201)]
    Created(u32),
}

fn main() {}
