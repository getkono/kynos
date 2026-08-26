//! The control for `macros/reply_unknown_member`: the same reply, differing
//! only in that the member is spelled the way the grammar defines it.

use kynos::Reply;

#[derive(Reply)]
enum ReportReply {
    #[reply(status = 200, description = "the finished report")]
    Ready(u32),
}

fn main() {}
