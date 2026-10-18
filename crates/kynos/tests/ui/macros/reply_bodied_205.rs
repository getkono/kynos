//! A server must not generate content in a 205 response (RFC 9110 §15.3.6),
//! so a variant answering with one holds no body.

use kynos::Reply;

#[derive(Reply)]
enum SubmitReply {
    #[reply(status = 205)]
    Reset(u32),
}

fn main() {}
