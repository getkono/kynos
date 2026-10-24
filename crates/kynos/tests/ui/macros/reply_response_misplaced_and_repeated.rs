//! `response` marks one variant as holding a response type, so on the enum it
//! marks none, on a unit variant there is no type to hold, and a second one on
//! a variant says nothing the first did not.

use kynos::{
    Reply,
    extract::body::json::Json,
    response::status::Created,
};

#[derive(Reply)]
#[reply(response)]
enum CreateReply {
    #[reply(status = 201)]
    Created(Created<Json<u32>>),
}

#[derive(Reply)]
enum DeleteReply {
    #[reply(status = 200, response)]
    Deleted,
}

#[derive(Reply)]
enum ImportReply {
    #[reply(status = 201, response)]
    #[reply(response)]
    Created(Created<Json<u32>>),
}

fn main() {}
