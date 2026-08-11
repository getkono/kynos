//! A 1xx is an interim response, and a handler returns the final one.

use kynos::Reply;

#[derive(Reply)]
enum UpgradeReply {
    #[reply(status = 100)]
    KeepGoing,
}

fn main() {}
