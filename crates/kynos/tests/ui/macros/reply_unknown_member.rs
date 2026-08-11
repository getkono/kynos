//! The grammar is closed, so a misspelled member is an error rather than a
//! silently dropped description.

use kynos::Reply;

#[derive(Reply)]
enum ReportReply {
    #[reply(status = 200, describtion = "the finished report")]
    Ready(u32),
}

fn main() {}
