//! Triggering graceful shutdown, and forcing it when a second signal arrives.

mod signal;

use std::{
    collections::HashMap,
    fmt,
    future::{Future, pending, poll_fn},
    io,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    task::{Context, Poll, Wake, Waker},
};

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
/// awaiting its clone hears the signal even where no server is serving, and a
/// clone left pending but no longer polled holds up none of the others.
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
///
/// Any polled clone advances the trigger, under a lock held only for that
/// synchronous poll, and the trigger's future is polled with a waker that
/// wakes every waiting clone. A clone whose task stops polling it therefore
/// holds nothing, and whichever clone is polled next advances the trigger.
struct Shared {
    trigger: Mutex<Trigger>,
    waiters: Arc<Waiters>,
    /// Wakes every [`waiters`](Self::waiters) entry; the trigger's future is
    /// always polled with it.
    waker: Waker,
}

/// The future a trigger waits on next, and how far it has advanced.
struct Trigger {
    stage: Stage,
    phase: Phase,
}

/// The future a trigger waits on next.
enum Stage {
    Begin(ShutdownFuture),
    Force(ForceFuture),
    Settled,
}

/// What every clone of a trigger has seen it do.
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

/// The wakers of the waits pending on a trigger, keyed by wait.
#[derive(Default)]
struct Waiters(Mutex<WaiterSet>);

#[derive(Default)]
struct WaiterSet {
    next: u64,
    wakers: HashMap<u64, Waker>,
}

impl Waiters {
    /// A key for a new wait, which [`Waiter`] releases when it is dropped.
    fn key(&self) -> u64 {
        let mut set = lock(&self.0);
        set.next += 1;
        set.next
    }

    fn register(&self, key: u64, waker: &Waker) {
        let mut set = lock(&self.0);
        match set.wakers.get_mut(&key) {
            Some(known) if known.will_wake(waker) => {}
            Some(known) => known.clone_from(waker),
            None => {
                set.wakers.insert(key, waker.clone());
            }
        }
    }
}

impl Wake for Waiters {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // Copied out first, so a waker re-entering the set cannot deadlock.
        let wakers: Vec<Waker> = lock(&self.0).wakers.values().cloned().collect();
        for waker in wakers {
            waker.wake();
        }
    }
}

/// One pending wait's place in [`Waiters`], released when the wait is
/// dropped, so a wait abandoned before the trigger fires leaves nothing.
struct Waiter<'a> {
    waiters: &'a Waiters,
    key: u64,
}

impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        lock(&self.waiters.0).wakers.remove(&self.key);
    }
}

/// Locks `mutex` through poisoning.
///
/// The waiter list is whole at every point a panic can leave it. A panic in
/// the trigger's future leaves its stage in place, so the next clone to poll
/// it panics too rather than reading a half-made transition: the phase is
/// only ever replaced whole, after the future it reports resolved.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// Polls the trigger until `reached` answers its phase, registering `cx`
    /// to be woken whenever the trigger may have advanced.
    fn poll_reach(
        &self,
        cx: &mut Context<'_>,
        key: u64,
        reached: fn(&Phase) -> Option<io::Result<()>>,
    ) -> Poll<io::Result<()>> {
        // Registered before the poll, so a wake during or after it reaches us.
        self.waiters.register(key, cx.waker());
        let mut trigger = lock(&self.trigger);
        let mut shared = Context::from_waker(&self.waker);
        loop {
            if let Some(answer) = reached(&trigger.phase) {
                return Poll::Ready(answer);
            }
            let (stage, phase) = match &mut trigger.stage {
                Stage::Begin(begin) => match begin.as_mut().poll(&mut shared) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(force)) => (Stage::Force(force), Phase::Begun),
                    Poll::Ready(Err(error)) => (Stage::Settled, Phase::Failed(Arc::new(error))),
                },
                Stage::Force(force) => match force.as_mut().poll(&mut shared) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(()) => (Stage::Settled, Phase::Forced),
                },
                Stage::Settled => unreachable!("a settled phase answers every wait"),
            };
            *trigger = Trigger { stage, phase };
            // The other clones wait on this transition.
            self.waker.wake_by_ref();
        }
    }
}

impl fmt::Debug for Shutdown {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Shutdown").finish_non_exhaustive()
    }
}

impl Shutdown {
    pub(in crate::server) fn new(future: ShutdownFuture) -> Self {
        let waiters = Arc::new(Waiters::default());
        Self {
            shared: Arc::new(Shared {
                trigger: Mutex::new(Trigger {
                    stage: Stage::Begin(future),
                    phase: Phase::Waiting,
                }),
                waker: Waker::from(Arc::clone(&waiters)),
                waiters,
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

    /// How many waits are registered to be woken.
    #[cfg(test)]
    pub(in crate::server) fn waiting(&self) -> usize {
        lock(&self.shared.waiters.0).wakers.len()
    }

    /// Waits until `reached` answers the trigger's phase, advancing the
    /// trigger on every poll.
    async fn reach(&self, reached: fn(&Phase) -> Option<io::Result<()>>) -> io::Result<()> {
        let waiter = Waiter {
            waiters: &self.shared.waiters,
            key: self.shared.waiters.key(),
        };
        poll_fn(|cx| self.shared.poll_reach(cx, waiter.key, reached)).await
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
