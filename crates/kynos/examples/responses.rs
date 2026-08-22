//! What a handler can return, and why none of it chooses a status at run time.
//!
//! ```text
//! cargo run -p kynos --example responses
//! ```
//!
//! Six things are worth noticing:
//!
//! * **Status is part of the return type.** There is no `StatusCode` to return
//!   and no builder to hand one to. `NoContent` is 204 because it is
//!   `NoContent`; `Created<T>` is 201 because it is `Created`. A status the
//!   description does not list is a status it is wrong about, and the only way
//!   to make that impossible is to take the choice out of the request path.
//! * **Several statuses means an enum.** `#[derive(Reply)]` declares a closed
//!   set, one variant per status. Two variants may not share a status, because
//!   the description keys them by status alone.
//! * **`Redirect<CODE>` refuses the wrong codes.** The bound is a witness
//!   implemented for exactly 301, 302, 303, 307 and 308, and both the trait and
//!   `()` are foreign to an application — so the set cannot be widened from
//!   outside and `Redirect::<304>` does not compile. That rules out writing 302
//!   where 307 was meant and silently changing the method on replay.
//! * **A download is a body type plus a header group.** Other frameworks hand
//!   you an `Attachment` type with a content-type setter on it. Here the media
//!   type is already the body's own type — `Binary<Pdf>` *is*
//!   `application/pdf` — so what is left to say is the disposition, and that is
//!   a header group like any other. Which is why it shows up in
//!   `Response.headers` without anyone writing it there, and why an interceptor
//!   that also set `Content-Disposition` would be a compile error rather than a
//!   response with two of them.
//! * **A resumable download is a `Range<T>` and a `Ranged<T>`.** The extractor
//!   is infallible, because RFC 9110 section 14.2 answers every unusable
//!   `Range` field by ignoring it — a malformed value or an unknown unit sends
//!   the whole representation and a 200, never a 400. What varies is the
//!   status, and none of it is chosen at run time: `Ranged<T>` declares the 200
//!   and the 206, and `RangeRejection` is the 416. Unlike `Accept`, the field
//!   *is* declared as a parameter, because a consumer that cannot see it does
//!   not know the operation resumes.
//! * **A location is a typed URI, not a format string.** Every route attribute
//!   emits `relative_uri`, taking exactly the path and query types that route
//!   extracts. A link that no longer matches its target is a compile error
//!   rather than a 404 a consumer finds first. It is named *relative* because a
//!   `Group` or `nest` prefix is applied while the router is built and is not
//!   visible to the attribute; every route here is mounted at the root, so
//!   there is no prefix to join.

use std::net::Ipv4Addr;

use kynos::{
    error::rejection::RangeRejection,
    extract::{
        body::binary::Binary,
        media::{OctetStream, Pdf},
    },
    prelude::*,
    response::{
        disposition::ContentDisposition,
        headers::WithHeaders,
        range::{Range, Ranged},
        status::{Accepted, Redirect},
    },
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

/// What `/users/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct UserPath {
    id: u64,
}

/// Rate-limit headers attached to a response.
///
/// The same derive `Headers<T>` uses while extracting. One declaration, both
/// directions — so a header this service sends is a header its description
/// lists, and `Response.headers` is complete by construction.
///
/// The names carry an `X-` prefix for the reason `docs/middleware.md` gives:
/// `RateLimit` and `RateLimit-Policy` belong to
/// `draft-ietf-httpapi-ratelimit-headers`, and this example is a hand-written
/// group rather than that draft. A derived group's names reach generated
/// clients, so squatting a spelling a working group is still revising is
/// expensive rather than cosmetic — and `middleware::rate_limit` refuses to do
/// it in exactly the same way.
#[allow(dead_code)]
#[derive(HeaderParams)]
struct Quota {
    #[header(rename = "X-Quota-Remaining")]
    remaining: u32,

    #[header(rename = "X-Quota-Reset")]
    reset: u32,
}

/// What creating a user can answer with.
///
/// Two statuses, so an enum. The 200 is not a failure — an identical user
/// already existed and the request was idempotent — which is why this is a
/// `Reply` and not an `ApiError`.
#[allow(dead_code)]
#[derive(Reply)]
enum CreateReply {
    #[reply(status = 201, description = "the user as stored")]
    Created(User),

    #[reply(status = 200, description = "an identical user already existed")]
    AlreadyExists(User),
}

/// Fetches one user.
///
/// A bare body type is 200. The wrapper is what changes it, so the common case
/// carries no ceremony.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Json<User> {
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// Creates a user.
///
/// `Created::at` takes the location as a required argument rather than an
/// option, because a 201 without one tells a client where the thing it just
/// made is: nowhere.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Created<Json<User>> {
    // The typed URI is the point. `get_user::relative_uri` takes exactly the
    // path type `get_user` extracts -- one argument, because that route
    // extracts one thing -- so changing the route's parameters breaks this line
    // rather than the link. `Location` is what lets the `http::Uri` it returns
    // arrive without a conversion here.
    Created::at(get_user::relative_uri(UserPath { id: user.id }), Json(user))
}

/// Creates a user, idempotently.
#[kynos::put("/users")]
async fn upsert_user(Json(user): Json<User>) -> CreateReply {
    // Which variant is the whole decision, and the status follows from it --
    // there is no second place where a code is chosen.
    if user.id == 0 {
        CreateReply::Created(user)
    } else {
        CreateReply::AlreadyExists(user)
    }
}

/// Queues a bulk import.
///
/// 202 says the work is accepted and not done. The body describes where to
/// watch it, which is the only thing a client can act on.
#[kynos::post("/users/imports")]
async fn queue_import(Json(user): Json<User>) -> Accepted<Json<User>> {
    Accepted::new(Json(user))
}

/// Lists users, with quota headers.
///
/// `WithHeaders` keeps the body's status — the headers are additional, not a
/// different response — and `H`'s derive is what puts them in `Response.headers`.
#[kynos::get("/users")]
async fn list_users() -> WithHeaders<Json<Vec<User>>, Quota> {
    let users = vec![User {
        id: 1,
        name: "Ada Lovelace".to_owned(),
    }];

    WithHeaders::new(
        Json(users),
        Quota {
            remaining: 99,
            reset: 60,
        },
    )
}

/// Serves the current invoice as a download.
///
/// The same `WithHeaders` the quota group uses, over a hand-written group
/// rather than a derived one: a `Content-Disposition` value is a grammar, not a
/// field per parameter. The accent is deliberate: the ASCII fallback cannot
/// hold it, so the field carries `filename` *and* `filename*`, which is what
/// RFC 6266 Appendix D asks a sender to do.
#[kynos::get("/invoices/current")]
async fn download_invoice() -> WithHeaders<Binary<Pdf>, ContentDisposition> {
    WithHeaders::new(
        Binary::new(&b"%PDF-1.7\n"[..]),
        ContentDisposition::attachment().filename("relevé.pdf"),
    )
}

/// Serves a recording, one part at a time if that is what was asked for.
///
/// `apply` is the whole of it: it resolves the field against the octets in
/// hand, slices them — refcounted, so nothing is copied — and returns the
/// response already knowing whether it is a 200 or a 206. A `Range` this
/// service cannot apply is not an error; it is the whole recording.
#[kynos::get("/recordings/current")]
async fn download_recording(
    range: Range<Binary<OctetStream>>,
) -> Result<Ranged<Binary<OctetStream>>, RangeRejection> {
    range.apply(Binary::new(
        &b"the first forty bytes of a recording ..."[..],
    ))
}

/// Redirects the legacy path to the current one.
///
/// 303 rather than 302: it says "see this other thing with GET", which is what
/// a moved collection means. `Redirect::<304>` would not compile, because 304
/// is not a redirect at all.
#[kynos::get("/accounts")]
async fn legacy_accounts() -> Redirect<303> {
    Redirect::to(list_users::relative_uri())
}

/// Deletes a user.
#[kynos::delete("/users/{id}")]
async fn delete_user(Path(path): Path<UserPath>) -> NoContent {
    println!("deleting {}", path.id);
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![
        get_user,
        create_user,
        upsert_user,
        queue_import,
        list_users,
        download_invoice,
        download_recording,
        legacy_accounts,
        delete_user,
    ]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
