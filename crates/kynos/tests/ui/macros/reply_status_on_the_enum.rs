//! A status on the enum would apply to every variant, which is the opposite of
//! what a closed set of responses is for.

use kynos::Reply;

#[derive(Reply)]
#[reply(status = 200)]
enum SearchReply {
    #[reply(status = 200)]
    Exact(u32),
    #[reply(status = 203)]
    Approximate(u32),
}

fn main() {}
