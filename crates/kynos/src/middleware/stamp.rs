//! A header group sent on every response, as an API version is.
//!
//! [`Stamp`] encodes the group once, when it is constructed, and writes the
//! stored fields onto every response passing through it, so a request pays
//! for inserting them and nothing else. Each value is an RFC 9110 section 5.5
//! field value, which a fixed `#[header(value = ...)]` is checked to be where
//! the group is declared.

use std::{convert::Infallible, marker::PhantomData};

use crate::{
    extract::params::header::EncodeHeaders,
    http::{self, HeaderName, HeaderValue},
    middleware::{Continued, Interceptor, MiddlewareError, Next, Placement},
};

/// Sends the header group `H` on every response it covers.
///
/// The group is declared as this interceptor's
/// [`Adds`](Interceptor::Adds), so it is described on every response each
/// covered operation declares ([`Placement::Every`]), and mounting it beside
/// anything else adding one of its fields is a compile error. A group of
/// fixed fields, each a `()` under `#[header(value = ...)]`, is described as
/// the one value it sends:
///
/// ```
/// use kynos::{
///     HeaderParams, Router,
///     middleware::stamp::Stamp,
/// };
///
/// pub const API_VERSION: &str = "2026-11-01";
///
/// #[derive(HeaderParams)]
/// struct Versioned {
///     /// The version of the API that answered.
///     #[header(rename = "X-Api-Version", value = API_VERSION)]
///     version: (),
/// }
///
/// # fn main() -> kynos::Result<()> {
/// let router = Router::<()>::new().intercept(Stamp::new(Versioned { version: () })?);
/// # let _ = router;
/// # Ok(())
/// # }
/// ```
///
/// # What carries the group
///
/// Every response the rest of the chain produces: the handler's, an
/// extractor's or guard's refusal, the short circuit of an interceptor mounted
/// inside this one, a panic an endpoint's own `catch_panics` recovered, and,
/// mounted on the router, its answer where no operation matched. A response answered
/// outside it never reaches it: an interceptor's mounted before it, and a
/// panic a router's or group's `catch_panics` recovers, around every
/// interceptor of both scopes. Mount it first, outermost.
///
/// The description files the group on every response an operation declares
/// in place, and leaves a shared `$ref` response, such as a Kynos refusal's,
/// as the reference it is.
///
/// Each field replaces whatever value the chain set, as
/// [`SecurityHeaders`](crate::middleware::security_headers::SecurityHeaders)'
/// fields do, so a handler's own value never survives beside the one the
/// description states; a field of a
/// [`REPEATABLE`](crate::extract::params::header::HeaderParams::REPEATABLE)
/// group is appended instead. A response whose handler declares the same
/// name is the exception: a placement never overwrites a header a response
/// already declares, so that response keeps the handler's schema in the
/// description while it carries the stamp's value.
///
/// The fields are those `headers` encoded when the stamp was constructed. A
/// value that differs between requests is not a stamp's to send: an
/// interceptor of its own computes it.
pub struct Stamp<H> {
    fields: Box<[(HeaderName, HeaderValue)]>,
    _group: PhantomData<fn() -> H>,
}

impl<H: EncodeHeaders> Stamp<H> {
    /// Encodes `headers` once, for every response this stamp covers.
    ///
    /// # Errors
    ///
    /// [`MiddlewareError::UnencodableStamp`] where a value the group holds is
    /// not a field value, which no response could then carry. A group of
    /// fixed fields alone never refuses, since each value was checked where
    /// the group is declared.
    // By value so the call reads `Stamp::new(Group { .. })`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(headers: H) -> Result<Self, MiddlewareError> {
        let fields = headers
            .encode()
            .map_err(|refused| MiddlewareError::UnencodableStamp(Box::new(refused)))?;
        Ok(Self {
            fields: fields.into_boxed_slice(),
            _group: PhantomData,
        })
    }
}

impl<H> Clone for Stamp<H> {
    fn clone(&self) -> Self {
        Self {
            fields: self.fields.clone(),
            _group: PhantomData,
        }
    }
}

impl<H> std::fmt::Debug for Stamp<H> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Stamp")
            .field("fields", &self.fields)
            .finish()
    }
}

impl<C, H> Interceptor<C> for Stamp<H>
where
    C: Sync + 'static,
    H: EncodeHeaders + 'static,
{
    type Reads = ();
    type Adds = H;

    /// Always continues: the group goes on whatever the chain returns.
    type Short = Infallible;

    const ADDS_ON: Placement = Placement::Every;

    const READS_BODY: bool = false;

    fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<H>, Infallible>> + Send {
        let _ = (reads, context);
        let run = next.run(request);

        async move { Ok(run.await.with_encoded::<H>(self.fields.iter().cloned())) }
    }
}
