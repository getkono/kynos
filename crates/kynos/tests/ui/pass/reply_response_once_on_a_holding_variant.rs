//! The control for `macros/reply_response_misplaced_and_repeated`: the same
//! replies, differing only in that `response` sits once on a variant holding a
//! response type, and the unit variant drops it.

use kynos::{
    Reply,
    extract::body::json::Json,
    response::status::Created,
};

#[derive(Reply)]
enum CreateReply {
    #[reply(status = 201, response)]
    Created(Created<Json<u32>>),
}

#[derive(Reply)]
enum DeleteReply {
    #[reply(status = 200)]
    Deleted,
}

#[derive(Reply)]
enum ImportReply {
    #[reply(status = 201, response)]
    Created(Created<Json<u32>>),
}

fn main() {}
