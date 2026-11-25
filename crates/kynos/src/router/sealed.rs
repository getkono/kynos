//! A finished router, with nothing left in its type but the context.

use kynos_openapi::{Document, SpecVersion, Violation};

use super::{Router, error::RouterError, install::catches, service::Service};
use crate::{
    error::Result,
    middleware::catch_panic::{Catch, PanicPolicy},
};

/// A router that is finished: it can be described and built, and nothing more
/// can be added to it.
///
/// [`Router::seal`] makes one. Its type names only the context, so a function
/// building an API writes `-> Sealed<App>` whatever it mounts, intercepts or
/// recovers, and borrows nothing from its arguments. A function returning a
/// [`Router`] instead leaves its other parameters opaque, and under edition
/// 2024 an opaque `impl Sized` captures the lifetime of every reference
/// argument:
///
/// ```no_run
/// use kynos::{Router, middleware::limits::body_size::BodySize, router::sealed::Sealed};
///
/// struct Options {
///     body_limit: u64,
/// }
///
/// fn api(options: &Options) -> Sealed<()> {
///     Router::<()>::new()
///         .catch_panics()
///         .intercept(BodySize::new(options.body_limit))
///         .seal()
/// }
///
/// # fn main() -> kynos::Result<()> {
/// // The options are a temporary, dropped before the router is used.
/// let api = api(&Options { body_limit: 64 * 1024 });
/// let document = api.openapi()?;
/// let service = api.build(())?;
/// # let _ = (document, service);
/// # Ok(())
/// # }
/// ```
///
/// Everything set before sealing is kept, the panic policy included, and the
/// [`Service`] it builds is the one the router would have built. Composition
/// is over once it is sealed: a `Sealed` has no method that adds to it, and
/// cannot be nested or merged, since no interceptor could be checked against
/// the stacks it erased.
///
/// ```compile_fail
/// use kynos::{Router, middleware::limits::body_size::BodySize};
///
/// let api = Router::<()>::new().seal().intercept(BodySize::new(64 * 1024));
/// ```
pub struct Sealed<C> {
    router: Erased<C>,
}

/// The router, at whichever of the two panic policies it was given.
enum Erased<C> {
    Propagate(Router<C>),
    Catch(Router<C, Catch>),
}

impl<C, P: PanicPolicy, I, S> Router<C, P, I, S> {
    /// Finishes the router, erasing every type parameter but the context, so
    /// a function can name what it returns; see [`Sealed`].
    ///
    /// Nothing is checked here: the checks run where the sealed router is
    /// described or built, as they would on this one.
    #[must_use]
    pub fn seal(self) -> Sealed<C> {
        let router = if catches::<P>() {
            Erased::Catch(self.retype())
        } else {
            Erased::Propagate(self.retype())
        };
        Sealed { router }
    }
}

impl<C> Sealed<C> {
    /// [`Router::validate`] on the router this sealed.
    ///
    /// # Errors
    ///
    /// As [`Router::validate`].
    pub fn validate(&self) -> Result<Vec<Violation<RouterError>>>
    where
        C: 'static,
    {
        match &self.router {
            Erased::Propagate(router) => router.validate(),
            Erased::Catch(router) => router.validate(),
        }
    }

    /// [`Router::openapi`] on the router this sealed.
    ///
    /// # Errors
    ///
    /// As [`Router::openapi`].
    pub fn openapi(&self) -> Result<Document>
    where
        C: 'static,
    {
        match &self.router {
            Erased::Propagate(router) => router.openapi(),
            Erased::Catch(router) => router.openapi(),
        }
    }

    /// [`Router::openapi_as`] on the router this sealed.
    ///
    /// # Errors
    ///
    /// As [`Router::openapi_as`].
    pub fn openapi_as(&self, version: SpecVersion) -> Result<Document>
    where
        C: 'static,
    {
        match &self.router {
            Erased::Propagate(router) => router.openapi_as(version),
            Erased::Catch(router) => router.openapi_as(version),
        }
    }

    /// [`Router::build`] on the router this sealed.
    ///
    /// # Errors
    ///
    /// As [`Router::build`].
    pub fn build(self, context: C) -> Result<Service<C>>
    where
        C: Send + Sync + 'static,
    {
        match self.router {
            Erased::Propagate(router) => router.build(context),
            Erased::Catch(router) => router.build(context),
        }
    }
}

#[cfg(feature = "unchecked")]
impl<C> Sealed<C> {
    /// [`Router::has_unchecked`] on the router this sealed: the gate a CI job
    /// asserts on survives sealing.
    #[must_use]
    pub fn has_unchecked(&self) -> bool {
        match &self.router {
            Erased::Propagate(router) => router.has_unchecked(),
            Erased::Catch(router) => router.has_unchecked(),
        }
    }

    /// [`Router::unchecked_reasons`] on the router this sealed.
    #[must_use]
    pub fn unchecked_reasons(&self) -> Vec<kynos_openapi::OpaqueReason> {
        match &self.router {
            Erased::Propagate(router) => router.unchecked_reasons(),
            Erased::Catch(router) => router.unchecked_reasons(),
        }
    }
}

impl<C> std::fmt::Debug for Sealed<C> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (router, catches): (&dyn std::fmt::Debug, bool) = match &self.router {
            Erased::Propagate(router) => (router, false),
            Erased::Catch(router) => (router, true),
        };
        formatter
            .debug_struct("Sealed")
            .field("router", router)
            .field("catch_panics", &catches)
            .finish()
    }
}
