//! The control for `macros/reply_status_out_of_range`: the same reply,
//! differing only in that the status names a final response.

use kynos::Reply;

#[derive(Reply)]
enum UpgradeReply {
    #[reply(status = 200)]
    Done,
}

fn main() {}
