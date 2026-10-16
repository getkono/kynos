//! The scope group's tables, and the catalog's stacks: the table behind
//! transparent layers at router scope, `/json/small` behind the same layers
//! at a narrower scope, with panic recovery, or watched by an observer.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use http_body::Frame;
use kynos::{
    http::{Request, Response},
    middleware::{Continued, Interceptor, Next, Observer, trace::Trace},
    prelude::*,
    router::{operation::Route, service::Service},
};
use tokio::io::DuplexStream;
use tracing::{
    Event, Metadata,
    instrument::WithSubscriber,
    span::{Attributes, Id, Record},
    subscriber::{DefaultGuard, Subscriber},
};

use super::{App, Small, built, json_small, rest, router, small};
use crate::driver::{Connecting, Drained, Runner, Served};

/// An interceptor that forwards and does nothing else, so that what a stack
/// costs is the chain's own machinery — `tests/alloc.rs`'s `Transparent`, for
/// its reason.
struct Transparent;

impl<C: Sync + 'static> Interceptor<C> for Transparent {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Infallible> {
        let _ = (reads, context);
        Ok(next.run(request).await)
    }
}

/// The table behind four transparent layers, written out because each
/// `intercept` returns a different `Router` type and a loop has none to
/// iterate at.
#[must_use]
pub(crate) fn layers_4() -> Box<dyn Served> {
    built(
        router()
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent),
    )
}

/// Behind eight, for the same reason.
#[must_use]
pub(crate) fn layers_8() -> Box<dyn Served> {
    built(
        router()
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent),
    )
}

/// `layers-4`'s four layers, on a group holding `/json/small` alone rather
/// than on the router.
pub(crate) fn group_layers_4() -> Box<dyn Served> {
    built(
        rest().group(
            Group::new("/")
                .intercept(Transparent)
                .intercept(Transparent)
                .intercept(Transparent)
                .intercept(Transparent)
                .mount(kynos::routes![json_small]),
        ),
    )
}

/// `layers-4`'s four layers, on the `/json/small` endpoint itself.
pub(crate) fn endpoint_layers_4() -> Box<dyn Served> {
    built(rest().mount(kynos::routes![
        json_small
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
            .intercept(Transparent)
    ]))
}

/// The whole table under router-scope panic recovery.
pub(crate) fn catching_router() -> Box<dyn Served> {
    built(router().catch_panics())
}

/// `json-small`'s handler, under its own operation id, asking for recovery on
/// that one endpoint.
#[kynos::get("/json/small", operation_id = "json_small", catch_panics)]
async fn json_small_catching() -> Json<Small> {
    Json(small())
}

/// The table with `/json/small` alone under panic recovery.
pub(crate) fn catching_endpoint() -> Box<dyn Served> {
    built(rest().mount(kynos::routes![json_small_catching]))
}

/// An observer that records nothing, so that what observing costs is the
/// observer chain's own machinery.
struct Unheard;

impl Observer<App> for Unheard {
    fn on_request(&self, request: &Request, route: Option<Route<'_>>, context: &App) {
        let _ = (request, route, context);
    }

    fn on_response(&self, response: &Response, route: Option<Route<'_>>, elapsed: Duration) {
        let _ = (response, route, elapsed);
    }
}

/// The table watched by an observer that does nothing.
pub(crate) fn observed() -> Box<dyn Served> {
    built(router().observe(Unheard))
}

/// The table under `Trace`'s defaults.
pub(crate) fn traced() -> Box<dyn Served> {
    built(router().observe(Trace::new()))
}

/// A subscriber that is interested in everything and keeps none of it, so
/// that what is counted is `tracing` building each span and event and handing
/// it over, and not what any real subscriber does with it.
struct Discarding;

impl Subscriber for Discarding {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let _ = metadata;
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let _ = span;
        Id::from_u64(1)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let _ = (span, values);
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        let _ = (span, follows);
    }

    fn event(&self, event: &Event<'_>) {
        let _ = event;
    }

    fn enter(&self, span: &Id) {
        let _ = span;
    }

    fn exit(&self, span: &Id) {
        let _ = span;
    }
}

/// A service served with [`Discarding`] as the thread's default subscriber,
/// installed when the service is built and removed when it is dropped, both
/// outside the region.
///
/// The service is held by type rather than boxed, so serving through this
/// is the same call `traced`'s service is. A connection outlives the thread
/// default, which [`connect`](Served::connect) consumes, so its future
/// carries [`Discarding`] as the default for each of its polls instead.
struct Subscribed {
    service: Service<App>,
    _default: DefaultGuard,
}

impl Served for Subscribed {
    fn serve(&self, runner: &Runner, request: Request, frames: Vec<Frame<Bytes>>) -> Drained {
        self.service.serve(runner, request, frames)
    }

    fn connect(self: Box<Self>, io: DuplexStream, peer: SocketAddr) -> Connecting {
        let Connecting { future, server } = Box::new(self.service).connect(io, peer);
        Connecting {
            future: Box::pin(future.with_subscriber(Discarding)),
            server,
        }
    }
}

/// [`traced`]'s table with a subscriber installed on the serving thread.
pub(crate) fn traced_subscribed() -> Box<dyn Served> {
    let service = router()
        .observe(Trace::new())
        .build(App::new())
        .expect("a describable router");
    Box::new(Subscribed {
        service,
        _default: tracing::subscriber::set_default(Discarding),
    })
}
