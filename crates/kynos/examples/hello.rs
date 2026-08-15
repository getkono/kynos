//! The whole path from an `async fn` to a served, described operation.
//!
//! ```text
//! cargo run -p kynos --example hello
//! ```
//!
//! Three things are worth noticing, because they are the framework's whole
//! argument:
//!
//! * Nothing here restates the signature. The path parameters, the request
//!   body, the response shape and the statuses each operation can produce all
//!   come from the types the server actually runs on, so there is no second
//!   declaration to drift from the first.
//! * `#[kynos::get("/users/{id}")]` checks at compile time that `UserPath`'s
//!   fields are exactly the template's variables, in order. Renaming one
//!   without the other does not build.
//! * `Router::openapi()` is the only way from this code to a description.
//!   There is no document to hand-edit and therefore none to forget.

use std::net::Ipv4Addr;

use kynos::{prelude::*, response::status::NoContent, server::Server};
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
#[derive(Schema, PathParams)]
struct UserPath {
    /// The identifier from the path.
    id: u64,
}

/// How a listing is paged.
#[allow(dead_code)]
#[derive(Schema, QueryParams)]
struct Page {
    /// Which page to return, counting from one.
    page: u32,
    /// How many users to return per page.
    per_page: u32,
}

/// Reports that the service is up.
///
/// The first paragraph becomes the operation's summary and the rest the
/// description — so the documentation a reader of this file sees and the
/// documentation a consumer of the API sees are the same words.
#[kynos::get("/health")]
async fn health() -> NoContent {
    NoContent
}

/// Lists users.
#[kynos::get("/users")]
async fn list_users(Query(page): Query<Page>) -> Json<Vec<User>> {
    println!("page {:?}, {:?} per page", page.page, page.per_page);

    Json(vec![User {
        id: 1,
        name: "Ada Lovelace".to_owned(),
    }])
}

/// Fetches one user.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Json<User> {
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// Creates a user.
///
/// The body extractor is last, because it is the one that consumes the request
/// body — a handler may have at most one, and every earlier argument reads only
/// the head.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Created<Json<User>> {
    // The typed URI takes exactly the path type `get_user` extracts, so
    // changing that route's parameters breaks this line rather than the link
    // it produces.
    Created::at(get_user::relative_uri(UserPath { id: user.id }), Json(user))
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router =
        Router::<()>::new().mount(kynos::routes![health, list_users, get_user, create_user]);

    // The description comes from the same types the server runs on, so this
    // cannot disagree with what the service does.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
