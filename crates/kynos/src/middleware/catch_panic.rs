//! Turning a panic into a documented response.

mod private {
    pub trait Sealed {}
}

/// A compile-time panic recovery policy.
///
/// This trait is sealed. Select a policy through
/// [`Router::catch_panics`](crate::router::Router::catch_panics),
/// [`Group::catch_panics`](crate::router::group::Group::catch_panics), or
/// [`EndpointBuilder::catch_panics`](crate::router::endpoint::EndpointBuilder::catch_panics)
/// rather than naming its implementations in application code.
pub trait PanicPolicy: private::Sealed + Send + Sync + 'static {}

/// Lets a panic continue unwinding.
///
/// This is the default policy and installs no recovery wrapper.
#[derive(Clone, Copy, Debug, Default)]
pub struct Propagate {
    _private: (),
}

impl private::Sealed for Propagate {}
impl PanicPolicy for Propagate {}

/// Converts a panic into a 500 problem document.
///
/// Selecting this policy contributes a 500 response to every covered
/// operation and requires the final binary to use `panic = "unwind"`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Catch {
    _private: (),
}

impl private::Sealed for Catch {}
impl PanicPolicy for Catch {}
