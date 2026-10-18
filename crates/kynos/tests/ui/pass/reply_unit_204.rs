//! The control for `macros/reply_bodied_204`: the same reply, differing only
//! in that the variant holds no body.

use kynos::Reply;

#[derive(Reply)]
enum DeleteReply {
    #[reply(status = 204)]
    Deleted,
}

fn main() {}
