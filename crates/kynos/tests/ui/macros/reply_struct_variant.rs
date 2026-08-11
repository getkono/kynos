//! A variant's fields are the response body, and a body is one described type.
//! An anonymous record has no name to register under and no `Schema`.

use kynos::Reply;

#[derive(Reply)]
enum CreateReply {
    #[reply(status = 201)]
    Created { id: u32, revision: u32 },
}

fn main() {}
