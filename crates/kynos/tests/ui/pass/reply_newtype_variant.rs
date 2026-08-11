//! The control for `macros/reply_struct_variant`: the same reply, differing
//! only in that the body is one named type rather than an anonymous record.

use kynos::{Reply, Schema};

#[derive(Schema)]
struct Created {
    id: u32,
    revision: u32,
}

#[derive(Reply)]
enum CreateReply {
    #[reply(status = 201)]
    Created(Created),
}

fn main() {}
