//! What an operation's guard leaves wrong with it: taking none among guarded
//! siblings, or not running ahead of an interceptor keyed on what it admits.
//!
//! Checked while the router is described, so `validate` reports what `build`
//! refuses; the placement it reads is the one `build.rs`'s `covering` makes.

use super::install::{error_at, path_location};
use super::{Arc, ErasedInterceptor, Mounted, Router, RouterError, Violation};

impl<C: 'static, P, I, S> Router<C, P, I, S> {
    /// What `mounted`'s guard leaves wrong with it, as violations located at
    /// the operation.
    pub(super) fn guard_refusals(
        &self,
        mounted: &Mounted<C>,
    ) -> impl Iterator<Item = Violation<RouterError>> {
        super::group::unguarded(mounted)
            .into_iter()
            .chain(guard_not_ahead(
                &self.interceptors,
                self.guard_first,
                mounted,
            ))
    }
}

/// The refusal of `mounted` where an interceptor covering it keys on what a
/// guard admitted and `covering` places no such guard ahead of it, located
/// at the operation: its first such interceptor, outermost first, the
/// endpoint's own last.
#[must_use]
fn guard_not_ahead<C: 'static>(
    router: &[Arc<dyn ErasedInterceptor<C>>],
    router_guard: Option<usize>,
    mounted: &Mounted<C>,
) -> Option<Violation<RouterError>> {
    /// An interceptor and its index among those `covering` places the
    /// guard among, before `covered` drops the ones not covering.
    struct Placed<'a, C>(usize, &'a Arc<dyn ErasedInterceptor<C>>);

    impl<C> AsRef<dyn ErasedInterceptor<C>> for Placed<'_, C> {
        fn as_ref(&self) -> &(dyn ErasedInterceptor<C> + 'static) {
            self.1.as_ref()
        }
    }

    let endpoint = &mounted.endpoint;
    let at = super::guard_point(router_guard, router.len(), mounted.guard_at);
    let taken = endpoint.guard_type();
    // The guard runs ahead of what sits at `at` and after it, where the
    // handler takes the very guard read.
    let ahead = |guard, index: usize| taken == Some(guard) && at.is_some_and(|at| index >= at);

    let placed = router
        .iter()
        .chain(&mounted.interceptors)
        .enumerate()
        .map(|(index, interceptor)| Placed(index, interceptor))
        .collect();
    let unseen = super::covered(placed, endpoint.reads_body())
        .into_iter()
        .find_map(|Placed(index, interceptor)| {
            let guard = interceptor.reads_admitted()?;
            (!ahead(guard, index)).then(|| (interceptor.name(), guard))
        })
        .or_else(|| {
            // Inside every scope's chain, so after any point one chose.
            let own = endpoint.admitted_reader()?;
            let inside = router.len() + mounted.interceptors.len();
            (!ahead(own.guard, inside)).then_some((own.interceptor, own.guard))
        });

    let (interceptor, guard) = unseen?;
    let method = endpoint.method();
    let path = mounted.path.as_str();
    Some(error_at(
        format!(
            "{}/{}",
            path_location(path),
            method.as_wire_str().to_lowercase()
        ),
        RouterError::GuardNotAhead {
            method,
            path: path.to_owned(),
            interceptor,
            guard: guard.name(),
        },
    ))
}
