//! The control for `macros/reply_bodied_205`: the same reply, differing only
//! in that the variant holds no body.

use kynos::Reply;

#[derive(Reply)]
enum SubmitReply {
    #[reply(status = 205)]
    Reset,
}

fn main() {}
