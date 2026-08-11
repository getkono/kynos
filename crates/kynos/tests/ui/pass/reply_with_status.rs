//! The control for `macros/reply_missing_status`: the same reply, differing
//! only in that the variant declares its status.

use kynos::Reply;

#[derive(Reply)]
enum CreateReply {
    #[reply(status = 201)]
    Created(u32),
}

fn main() {}
