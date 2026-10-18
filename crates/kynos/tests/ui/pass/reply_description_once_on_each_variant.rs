//! The control for `macros/reply_misplaced_and_repeated_description`: the same
//! replies, differing only in that each description sits on a variant, once.

use kynos::Reply;

#[derive(Reply)]
enum ReportReply {
    #[reply(status = 200, description = "what reporting answers")]
    Ready(u32),
}

#[derive(Reply)]
enum ExportReply {
    #[reply(status = 200, description = "the finished export")]
    Ready(u32),
}

fn main() {}
