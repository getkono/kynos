//! Response compression.
//!
//! Out-of-document: content coding is transport, and OpenAPI does not model it.

use std::convert::Infallible;

use crate::{
    extract::params::header::HeaderParams,
    http,
    middleware::{Continued, Interceptor, Next},
};

/// What compression sets on a response it encoded.
///
/// `DESCRIBED` is `false`: both headers are defined by HTTP itself and handled
/// by every client without being told. Declaring the names is still what stops
/// a second interceptor touching them -- the check does not care whether a
/// consumer wanted to hear about them.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContentEncoding;

impl HeaderParams for ContentEncoding {
    const NAMES: &'static [&'static str] = &["content-encoding", "vary"];
    const DESCRIBED: bool = false;

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        todo!()
    }
}

/// Compresses responses when the client accepts it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compression {
    _private: (),
}

impl Compression {
    /// Enables every compiled-in algorithm.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Skips responses smaller than `bytes`.
    #[must_use]
    pub fn min_size(self, bytes: u64) -> Self {
        let _ = bytes;
        todo!()
    }
}

impl<C: Sync + 'static> Interceptor<C> for Compression {
    type Reads = ();
    type Adds = ContentEncoding;

    /// Always continues: compression re-encodes a response, never replaces it.
    type Short = Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<ContentEncoding>, Infallible> {
        let _ = (request, reads, context, next);
        todo!()
    }
}
