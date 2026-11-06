//! A variant holding a response type compiles when a `macro_rules!` adds the
//! derive and is handed the held type: the derive's own bindings keep its
//! hygiene context however the type's tokens arrive.

use kynos::{Reply, extract::body::json::Json, response::status::Created};

macro_rules! reply {
    (enum $name:ident { $($body:tt)* }) => {
        #[derive(Reply)]
        enum $name { $($body)* }
    };
}

reply! {
    enum CreateReply {
        #[reply(status = 201, response)]
        Created(Created<Json<u32>>),
    }
}

fn main() {}
