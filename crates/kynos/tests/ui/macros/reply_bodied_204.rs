//! A 204 response cannot contain content (RFC 9110 §15.3.5), so a variant
//! answering with one holds no body.

use kynos::Reply;

#[derive(Reply)]
enum DeleteReply {
    #[reply(status = 204)]
    Deleted(u32),
}

fn main() {}
