//! Dependency injection: what a handler needs, proved at compile time.
//!
//! ```text
//! cargo run -p kynos --example state
//! ```
//!
//! The application's own struct *is* the context. `#[derive(Provider)]` emits
//! one `Provides` implementation per field, a handler asks for what it needs
//! with `Inject<T>`, and the two meet at `mount` — which is where the context
//! type first becomes concrete, and therefore where asking for something the
//! context does not have stops compiling.
//!
//! The comparison worth drawing: axum, actix-web and poem all resolve erased
//! state at run time and panic when it is absent, and salvo's `Depot` is keyed
//! by string. Here there is nothing to look up. Try adding
//!
//! ```ignore
//! #[kynos::get("/mailer")]
//! async fn broken(Inject(mailer): Inject<Mailer>) -> NoContent { NoContent }
//! ```
//!
//! and mounting it: the build fails at the `mount` call with "the context `App`
//! provides no `Mailer`", naming both types and the field to add.

use std::{net::Ipv4Addr, sync::Arc};

use kynos::{prelude::*, response::status::NoContent, server::Server};

/// A database handle. Cheap to clone, which is what an injected value should
/// be: one is handed out per request.
#[derive(Clone)]
struct Pool {
    _connections: Arc<()>,
}

/// A cache handle.
#[derive(Clone)]
struct Cache {
    _entries: Arc<()>,
}

/// The application context.
///
/// Every provided field's type must be `Clone`. `#[provide(skip)]` opts a field
/// out, because not everything an application holds is a dependency.
#[derive(Provider)]
struct App {
    pool: Pool,
    cache: Cache,
    #[provide(skip)]
    #[allow(dead_code)]
    name: &'static str,
}

/// Reads through the cache, falling back to the pool.
///
/// Two injected values, in any order, alongside anything the request carries.
/// Neither appears in the description: application state has no effect on the
/// wire, so `Inject<T>` describes nothing — and says so, rather than being
/// silently skipped.
#[kynos::get("/users")]
async fn list_users(Inject(pool): Inject<Pool>, Inject(cache): Inject<Cache>) -> NoContent {
    let _ = (pool, cache);
    NoContent
}

/// Acquisition that can fail is not injection.
///
/// Inject the *handle* and check it out here, where the failure lands in the
/// return type and therefore in the description. A provider that could fail
/// would produce a response no operation declares — which is the one thing this
/// framework exists to prevent.
#[kynos::get("/health")]
async fn health(Inject(pool): Inject<Pool>) -> kynos::Result<NoContent, Problem> {
    let _ = pool;
    Ok(NoContent)
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let context = App {
        pool: Pool {
            _connections: Arc::new(()),
        },
        cache: Cache {
            _entries: Arc::new(()),
        },
        name: "orders",
    };

    let service = Router::<App>::new()
        .mount(kynos::routes![list_users, health])
        .build(context)?;

    Server::new(service)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
