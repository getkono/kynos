//! What a route does when it cannot succeed.
//!
//! ```text
//! cargo run -p kynos --example errors
//! ```
//!
//! Five things are worth noticing, because together they are the whole error
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
//! * **A rejection is one type per extractor, and each names only its own
//!   statuses.** `Path<T>` rejects with [`PathRejection`], `Query<T>` with
//!   [`QueryRejection`], `Json<T>` with [`BodyRejection`]. A single shared union
//!   would be sound and would still make a handler that reads one path
//!   parameter advertise the 401 it can never answer — which a client generator
//!   turns into dead retry logic.
//! * **Four of the responses below come from no handler at all.** A fallback
//!   policy produces the 404 and the 405, `catch_panics` produces the 500, and
//!   an interceptor produces the 503. Each reaches the description by a
//!   different route, and not one of them appears in a signature.
//!
//! [`middleware.rs`](middleware.rs) covers contributions properly, and
//! [`composition.rs`](composition.rs) covers the fallback policies as router
//! structure. They appear here because a reader asking "where did this status
//! come from" should find every answer in one file.
//!
//! [`PathRejection`]: kynos::error::rejection::PathRejection
//! [`QueryRejection`]: kynos::error::rejection::QueryRejection
//! [`BodyRejection`]: kynos::error::rejection::BodyRejection

use std::net::Ipv4Addr;

use kynos::{
    http,
    middleware::{Continued, Interceptor, Next},
    prelude::*,
    router::policy::FallbackPolicy,
    server::Server,
};
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

/// How the user list is paged.
///
/// A second fallible extractor, and the reason 400 is listed once rather than
/// twice: `QueryRejection` and `PathRejection` are different types that happen
/// to agree on a status, and an operation's `responses` is a union rather than a
/// list.
#[allow(dead_code)]
#[derive(Schema, QueryParams)]
struct Page {
    /// Where to resume from.
    after: Option<u64>,

    /// How many to return.
    #[schema(minimum = 1, maximum = 100)]
    limit: u32,
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
///
/// `OverQuota` reaches a client as this, which is the shape every error in the
/// service takes:
///
/// ```json
/// {
///   "type": "https://errors.example.com/over-quota",
///   "title": "Quota exceeded",
///   "status": 507,
///   "detail": "the store is over its 10000 row budget",
///   "limit": 10000
/// }
/// ```
///
/// `detail` is the `Display` output, so `thiserror` writes it once. `limit` sits
/// beside the registered members rather than nested under them, which is what
/// RFC 9457 calls an extension.
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

/// Refuses everything while the store is being migrated.
///
/// The fourth way a status reaches an operation. Because this can answer before
/// the handler runs, every operation it covers must document that it can — which
/// is what the contribution does, and what a `tower::Layer` has no way to say.
///
/// [`middleware.rs`](middleware.rs) is where interceptors are the subject; this
/// one exists so that every status this service can answer with is visible in
/// one file.
struct MaintenanceWindow {
    draining: bool,
}

/// The 503 the window answers with.
///
/// An `ApiError` like every other failure in this file, so the status no
/// handler mentions still reaches a client in the shape all the others do.
#[derive(Debug, thiserror::Error, ApiError)]
#[error("the store is being migrated")]
#[problem(base = "https://errors.example.com/", status = 503)]
struct Draining;

impl<C: Sync + 'static> Interceptor<C> for MaintenanceWindow {
    type Reads = ();
    type Adds = ();

    /// The declaration and the answer, in one type. There is no second place
    /// to say 503, so there is nowhere for the two to disagree.
    type Short = Draining;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Draining> {
        let _ = (context, reads);

        if self.draining {
            return Err(Draining);
        }

        Ok(next.run(request).await)
    }
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

/// Lists users.
///
/// `catch_panics` puts a recovery boundary around this one operation and
/// contributes the 500 that boundary can produce. Without it a panic is not a
/// documented response — it is the connection ending, which no description can
/// express and no client can tell apart from a network failure.
#[kynos::get("/users", catch_panics)]
async fn list_users(Query(page): Query<Page>) -> Result<Json<Vec<User>>, StoreError> {
    let _ = page;
    Ok(Json(Vec::new()))
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

// The signature a real store would have -- it takes the row it is storing.
// This stub always fails, so it never reaches the move, but narrowing the
// parameter to fit the stub would make the example misleading.
#[allow(clippy::needless_pass_by_value)]
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
    let router = Router::<()>::new()
        .intercept(MaintenanceWindow { draining: false })
        // Both are already `Problem`; naming them is what makes it deliberate.
        // A client that meets one error shape everywhere can parse errors once,
        // and the two responses no operation describes are exactly the ones a
        // client that has gone wrong will meet first.
        .not_found(FallbackPolicy::Problem)
        .method_not_allowed(FallbackPolicy::Problem)
        .mount(kynos::routes![get_user, list_users, create_user]);

    // Every status any of this can answer with is in here, and none of them was
    // written down twice: 404, 409 and 507 from `StoreError`, 400 from the
    // fallible extractors, 503 from the interceptor, and 500 from the recovery
    // boundary on `list_users`.
    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
