//! Triggering graceful shutdown, and forcing it when a second signal arrives.

mod signal;

use std::{
    fmt,
    future::{Future, pending},
    io,
    pin::Pin,
    sync::Arc,
};

use tokio::sync::{Mutex, watch};

use crate::server::shutdown::signal::{platform_signals, repeatable_ctrl_c};

pub(in crate::server) type ForceFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
/// Resolves when shutdown begins, to the future that forces it.
pub(in crate::server) type ShutdownFuture =
    Pin<Box<dyn Future<Output = io::Result<ForceFuture>> + Send + 'static>>;

/// A trigger that begins graceful shutdown.
///
/// Built-in operating-system triggers force any remaining work to stop when a
/// second matching signal arrives. Custom triggers rely on the server's drain
/// deadline for forced termination.
///
/// Installing a Tokio operating-system signal listener replaces that signal's
/// default process behavior for the rest of the process lifetime. Kynos keeps
/// its listeners alive through the drain so repeated signals remain effective.
///
/// Every clone is the same trigger, not a copy of it: one signal begins
/// shutdown for each server given a clone and each task awaiting
/// [`begun`](Self::begun), and a second forces it for all of them. Whichever
/// clone is awaited first installs the listeners, so a background task
/// awaiting its clone hears the signal even where no server is serving.
///
/// ```no_run
/// use kynos::{
///     Router,
///     server::{Server, shutdown::Shutdown},
/// };
///
/// # #[tokio::main]
/// # async fn main() -> kynos::Result<()> {
/// let shutdown = Shutdown::signals();
/// let jobs = shutdown.clone();
/// tokio::spawn(async move {
///     // ...claim and run jobs until...
///     if jobs.begun().await.is_ok() {
///         // ...stop claiming, and finish what is in flight...
///     }
/// });
///
/// let operator = Server::new(Router::<()>::new().build(())?)
///     .bind(("127.0.0.1", 9000))
///     .graceful_shutdown(shutdown.clone())
///     .prepare()
///     .await?
///     .spawn();
/// Server::new(Router::<()>::new().build(())?)
///     .bind(("0.0.0.0", 3000))
///     .graceful_shutdown(shutdown)
///     .serve()
///     .await?;
/// operator.shutdown().await
/// # }
/// ```
#[derive(Clone)]
pub struct Shutdown {
    shared: Arc<Shared>,
}

/// The one trigger every clone of a [`Shutdown`] observes.
struct Shared {
    /// The trigger itself, advanced by whichever clone holds the lock while
    /// the others watch [`phase`](Self::phase).
    stage: Mutex<Stage>,
    /// How far the trigger has advanced, written only under `stage`'s lock.
    phase: watch::Sender<Phase>,
}

/// The future a trigger waits on next.
enum Stage {
    Begin(ShutdownFuture),
    Force(ForceFuture),
    Settled,
}

/// What every clone of a trigger has seen it do.
#[derive(Clone)]
enum Phase {
    Waiting,
    Begun,
    Forced,
    /// Shared because `io::Error` is not `Clone`, so each clone reports it.
    Failed(Arc<io::Error>),
}

impl Phase {
    fn begun(&self) -> Option<io::Result<()>> {
        match self {
            Self::Waiting => None,
            Self::Begun | Self::Forced => Some(Ok(())),
            Self::Failed(error) => Some(Err(failure(error))),
        }
    }

    fn forced(&self) -> Option<io::Result<()>> {
        match self {
            Self::Waiting | Self::Begun => None,
            Self::Forced => Some(Ok(())),
            Self::Failed(error) => Some(Err(failure(error))),
        }
    }
}

/// A clone's copy of the trigger's failure, keeping its kind and message.
fn failure(error: &Arc<io::Error>) -> io::Error {
    io::Error::new(error.kind(), Arc::clone(error))
}

impl Shared {
    /// Advances `stage` past the future it waits on.
    async fn advance(&self, stage: &mut Stage) {
        match stage {
            Stage::Begin(begin) => match begin.await {
                Ok(force) => {
                    *stage = Stage::Force(force);
                    self.phase.send_replace(Phase::Begun);
                }
                Err(error) => {
                    *stage = Stage::Settled;
                    self.phase.send_replace(Phase::Failed(Arc::new(error)));
                }
            },
            Stage::Force(force) => {
                force.await;
                *stage = Stage::Settled;
                self.phase.send_replace(Phase::Forced);
            }
            // Both settled phases answer every wait before it advances.
            Stage::Settled => {}
        }
    }
}

// A panic in the trigger's future leaves its stage in place, so the next clone
// to advance it panics too rather than reading a half-made transition: the
// phase is only ever replaced whole, after the future it reports resolved.
impl std::panic::UnwindSafe for Shutdown {}
impl std::panic::RefUnwindSafe for Shutdown {}

impl fmt::Debug for Shutdown {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Shutdown").finish_non_exhaustive()
    }
}

impl Shutdown {
    pub(in crate::server) fn new(future: ShutdownFuture) -> Self {
        Self {
            shared: Arc::new(Shared {
                stage: Mutex::new(Stage::Begin(future)),
                phase: watch::Sender::new(Phase::Waiting),
            }),
        }
    }

    /// The future that begins shutdown, and resolves to the one that forces it.
    pub(in crate::server) fn into_future(self) -> ShutdownFuture {
        Box::pin(async move {
            self.begun().await?;
            Ok(Box::pin(async move {
                // A trigger fails only before it begins, so this is `Ok`.
                let _ = self.forced().await;
            }) as ForceFuture)
        })
    }

    /// Resolves once shutdown has begun, and at once if it already has.
    ///
    /// Awaiting it drives the trigger, as a server given a clone does, so it
    /// needs no server to resolve. It resolves when the trigger fires, not
    /// when a server's drain begins: a
    /// [deregistration delay](crate::server::Server::deregistration_delay)
    /// postpones only that server's drain, which
    /// [`Draining`](crate::server::lifecycle::Draining) reports.
    ///
    /// # Errors
    ///
    /// The trigger's failure, as an operating-system signal listener that
    /// cannot be installed, which every clone reports alike.
    pub async fn begun(&self) -> io::Result<()> {
        self.reach(Phase::begun).await
    }

    /// Resolves once shutdown is forced, and at once if it already has been.
    ///
    /// A built-in trigger forces on its second signal. A [`Shutdown::on`]
    /// trigger never forces, so this never resolves for one.
    ///
    /// # Errors
    ///
    /// The trigger's failure, as [`begun`](Self::begun) reports it.
    pub async fn forced(&self) -> io::Result<()> {
        self.reach(Phase::forced).await
    }

    /// Waits until `reached` answers the trigger's phase, advancing the
    /// trigger whenever no other clone is.
    async fn reach(&self, reached: fn(&Phase) -> Option<io::Result<()>>) -> io::Result<()> {
        let mut phase = self.shared.phase.subscribe();
        loop {
            let answer = reached(&phase.borrow_and_update());
            if let Some(answer) = answer {
                return answer;
            }
            tokio::select! {
                biased;
                // `self` holds the sender, so the channel never closes.
                _ = phase.changed() => {}
                mut stage = self.shared.stage.lock() => {
                    // The phase changes only under this lock, so it is current.
                    let answer = reached(&self.shared.phase.borrow());
                    if answer.is_none() {
                        self.shared.advance(&mut stage).await;
                    }
                }
            }
        }
    }

    /// Resolves on the first Ctrl-C and forces shutdown on the second.
    #[must_use]
    pub fn ctrl_c() -> Self {
        Self::new(repeatable_ctrl_c())
    }

    /// Resolves on the platform's conventional termination signals.
    ///
    /// Unix listens for `SIGINT` and `SIGTERM`. Windows listens for Ctrl-C,
    /// Ctrl-Break, console close, logoff, and system shutdown events. A second
    /// watched event forces shutdown immediately.
    #[must_use]
    pub fn signals() -> Self {
        Self::new(platform_signals())
    }

    /// Resolves successfully when `future` does.
    ///
    /// Custom triggers do not provide an early force signal. The configured
    /// shutdown timeout remains the upper bound on their drain.
    #[must_use]
    pub fn on(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self::new(Box::pin(async move {
            future.await;
            Ok(Box::pin(pending()) as ForceFuture)
        }))
    }

    /// Begins shutdown on `configured` or on `trigger`, whichever resolves
    /// first.
    ///
    /// Where `trigger` begins it, `configured` keeps its own meaning through
    /// the drain: its first resolution asks for the shutdown already under
    /// way, and the force it then yields forces, so a built-in trigger still
    /// forces on its second signal and a custom one never does. Polling
    /// `configured` first surfaces a signal that cannot be registered even
    /// where `trigger` is already resolved.
    pub(in crate::server) fn or_on(
        configured: Option<Self>,
        trigger: impl Future<Output = ()> + Send + 'static,
    ) -> Self {
        let Some(configured) = configured else {
            return Self::on(trigger);
        };
        let mut future = configured.into_future();
        Self::new(Box::pin(async move {
            let trigger = std::pin::pin!(trigger);
            tokio::select! {
                biased;
                begun = &mut future => begun,
                () = trigger => Ok(Box::pin(async move {
                    match future.await {
                        Ok(force) => force.await,
                        Err(_) => pending().await,
                    }
                }) as ForceFuture),
            }
        }))
    }
}
