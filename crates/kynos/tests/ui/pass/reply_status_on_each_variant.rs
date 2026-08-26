//! The control for `macros/reply_status_on_the_enum`: the same reply,
//! differing only in that each status is written where the variant that
//! produces it is.

use kynos::Reply;

#[derive(Reply)]
enum SearchReply {
    #[reply(status = 200)]
    Exact(u32),
    #[reply(status = 203)]
    Approximate(u32),
}

fn main() {}
