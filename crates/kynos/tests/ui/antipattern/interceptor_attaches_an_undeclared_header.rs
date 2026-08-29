//! Attaching a header group the interceptor never declared does not compile.
//!
//! The mirror of `interceptor_adds_what_it_declares`: that one declares a group
//! and never attaches it, this one attaches a group and never declares it.
//!
//! `with_headers` writes the group and then relabels the `Continued`, so a
//! second call relabels it back to the declared type with the first group's
//! fields already on the response. `Adds` is `()` here, and `Adds::NAMES` is
//! what `CompatibleWith` compares -- so `x-stamp` would reach the wire under a
//! declaration naming no header at all, and a second interceptor adding
//! `x-stamp` would still compile.

use std::convert::Infallible;

use kynos::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderName, HeaderValue, Request},
    middleware::{Continued, Interceptor, Next},
};

struct Stamp(HeaderValue);

impl HeaderParams for Stamp {
    const NAMES: &'static [&'static str] = &["x-stamp"];
}

impl EncodeHeaders for Stamp {
    fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
        vec![(HeaderName::from_static("x-stamp"), self.0.clone())]
    }
}

struct Smuggling;

impl<C: Sync + 'static> Interceptor<C> for Smuggling {
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
        Ok(next
            .run(request)
            .await
            .with_headers(Stamp(HeaderValue::from_static("1")))
            .with_headers(()))
    }
}

fn main() {}
