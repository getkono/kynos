//! The control for `macros/reply_status_repeated`: the same reply, differing
//! only in that the two variants answer with different statuses.

use kynos::Reply;

#[derive(Reply)]
enum UploadReply {
    #[reply(status = 202)]
    Queued(u32),
    #[reply(status = 200)]
    AlreadyStored(u32),
}

fn main() {}
