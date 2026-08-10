//! The request and response body.
//!
//! Opaque by design, and the one place the erased body type is named. A body is
//! consumed through a typed extractor, so nothing above this file needs to know
//! what it is erased into.

use std::{
    error::Error as StdError,
    fmt,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::{BodyExt, Empty, Full, combinators::UnsyncBoxBody};

type BoxError = Box<dyn StdError + Send + Sync>;

/// The request body.
///
/// Opaque by design. Bodies are consumed through a typed extractor such as
/// [`Json`](crate::extract::body::json::Json), never read directly.
pub struct Body {
    inner: Mutex<UnsyncBoxBody<Bytes, BoxError>>,
}

impl Body {
    /// An empty body.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            inner: Mutex::new(Empty::new().map_err(|never| match never {}).boxed_unsync()),
        }
    }

    /// A body holding exactly these bytes.
    #[must_use]
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self {
            inner: Mutex::new(
                Full::new(bytes)
                    .map_err(|never| match never {})
                    .boxed_unsync(),
            ),
        }
    }

    #[cfg(feature = "server")]
    pub(crate) fn from_incoming(body: hyper::body::Incoming) -> Self {
        Self {
            inner: Mutex::new(body.map_err(Into::into).boxed_unsync()),
        }
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Body").finish_non_exhaustive()
    }
}

impl HttpBody for Body {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        // `UnsyncBoxBody` is already pinned internally and moving its pointer
        // does not move the erased body.
        let inner = self
            .get_mut()
            .inner
            .get_mut()
            .expect("a mutably borrowed body mutex cannot be poisoned");
        Pin::new(inner).poll_frame(context)
    }

    fn is_end_stream(&self) -> bool {
        self.inner
            .lock()
            .expect("body mutex poisoned")
            .is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.lock().expect("body mutex poisoned").size_hint()
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}
