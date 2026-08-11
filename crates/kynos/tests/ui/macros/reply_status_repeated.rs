//! Two variants under one status are two bodies the description would have to
//! key by the same code, and a `Reply` is one variant per status.

use kynos::Reply;

#[derive(Reply)]
enum UploadReply {
    #[reply(status = 202)]
    Queued(u32),
    #[reply(status = 202)]
    AlreadyQueued(u32),
}

fn main() {}
