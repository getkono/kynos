//! What a route does when it cannot succeed.
//!
//! ```text
//! cargo run -p kynos --example errors
//! ```
//!
//! Three things are worth noticing, because together they are the whole error
//! model:
//!
//! * **Nothing lists the statuses.** `StoreError` says 404, 409 and 507 by
//!   deriving them; `Path<UserPath>` says 400 by being fallible; the
//!   operation's `responses` is the union. There is no `responses(...)`
//!   attribute to keep in step with the code, because there is nothing to keep
//!   in step.
//! * **`?` works the way it does everywhere else.** `RowMissing` is the store's
//!   own failure and knows nothing about HTTP. `#[from]` turns it into the
//!   variant that does.
//! * **The status is never chosen at run time.** `Problem` — the RFC 9457
//!   document that actually goes on the wire — carries its status in a field
//!   and therefore cannot be returned from a handler at all. Naming an error
//!   type is what makes the status set a `const`.

use std::net::Ipv4Addr;

use kynos::{prelude::*, server::Server};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    /// The user's identifier.
    id: u64,
    /// The user's display name.
    name: String,
}

/// What `/users/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct UserPath {
    /// The identifier from the path.
    id: u64,
}

/// The store's own failure.
///
/// Deliberately says nothing about HTTP: a storage layer that knows about
/// status codes is one that cannot be reused, and one whose tests need a
/// request.
#[derive(Debug, thiserror::Error)]
#[error("no row with id {0}")]
struct RowMissing(u64);

/// What the API says when the store cannot do what was asked.
///
/// One variant per failure a consumer should be able to tell apart. The
/// `base` is the prefix every variant's `type` URI shares, so a client can
/// branch on a stable identifier rather than on prose.
#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    /// `#[from]` is what makes the `?` in `get_user` compile.
    #[error(transparent)]
    #[problem(status = 404, title = "User not found")]
    NotFound(#[from] RowMissing),

    #[error("that email is already registered")]
    #[problem(status = 409)]
    EmailTaken,

    #[error("the store is over its {limit} row budget")]
    #[problem(status = 507, title = "Quota exceeded")]
    OverQuota {
        /// Published as an extension member, because a client can act on it.
        #[problem(extension)]
        limit: u64,
        /// Not published. A field is on the wire only when it says so, since a
        /// variant carries whatever the error site had to hand.
        shard: String,
    },
}

/// Fetches one user.
///
/// The `?` converts `RowMissing` into `StoreError::NotFound`, and the operation
/// documents 404 because of it. It also documents 400, because `Path<UserPath>`
/// can reject a parameter that will not parse — a response no line of this file
/// mentions and every consumer will nonetheless see.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Result<Json<User>, StoreError> {
    let user = load(path.id)?;
    Ok(Json(user))
}

/// Creates a user.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Result<Created<Json<User>>, StoreError> {
    let stored = insert(user)?;
    Ok(Created::at(format!("/users/{}", stored.id), Json(stored)))
}

fn load(id: u64) -> Result<User, RowMissing> {
    Err(RowMissing(id))
}

fn insert(user: User) -> Result<User, StoreError> {
    if user.id == 0 {
        return Err(StoreError::EmailTaken);
    }

    Err(StoreError::OverQuota {
        limit: 10_000,
        shard: "eu-west-1".to_owned(),
    })
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![get_user, create_user]);

    // Every status either handler can answer with is in here, and none of them
    // was written down twice.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
