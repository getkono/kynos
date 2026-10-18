//! A 304 response cannot contain content (RFC 9110 §15.4.5), so a variant
//! answering with one holds no body.

use kynos::Reply;

#[derive(Reply)]
enum FetchReply {
    #[reply(status = 304)]
    NotModified(u32),
}

fn main() {}
