//! The control for `macros/reply_bodied_304`: the same reply, differing only
//! in that the variant holds no body.

use kynos::Reply;

#[derive(Reply)]
enum FetchReply {
    #[reply(status = 304)]
    NotModified,
}

fn main() {}
