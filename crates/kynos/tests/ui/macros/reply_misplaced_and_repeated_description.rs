//! A `#[reply(...)]` description describes one response, so on the enum it
//! described none and was dropped, and a second one on a variant silently
//! replaced the first.

use kynos::Reply;

#[derive(Reply)]
#[reply(description = "what reporting answers")]
enum ReportReply {
    #[reply(status = 200)]
    Ready(u32),
}

#[derive(Reply)]
enum ExportReply {
    #[reply(status = 200, description = "the finished export")]
    #[reply(description = "the export")]
    Ready(u32),
}

fn main() {}
