//! The adapter behind [`Body::watching`](super::Body::watching).

use std::{
    mem,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::combinators::UnsyncBoxBody;

use super::{BoxError, Delivery};

/// A body that reports how it ended.
///
/// Not generic over the callback. A boxed `FnOnce` is unconditionally [`Unpin`]
/// whatever it captures, which is what lets `poll_frame` reach its fields
/// through [`Pin::get_mut`] -- `unsafe` is forbidden here, so a hand-written
/// projection is not available.
pub(super) struct Watched {
    pub(super) inner: UnsyncBoxBody<Bytes, BoxError>,
    /// `None` once the report has been made, which is what makes it once.
    pub(super) report: Option<Box<dyn FnOnce(Delivery) + Send>>,
}

impl Watched {
    /// Reports `delivery`, unless something already reported.
    fn report(&mut self, delivery: Delivery) {
        if let Some(report) = self.report.take() {
            report(delivery);
        }
    }
}

impl HttpBody for Watched {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_frame(context);

        match &polled {
            // Nothing left: everything the body had was yielded.
            Poll::Ready(None) => this.report(Delivery::Complete),
            // A body may declare its end on the frame that carries the last of
            // it rather than on a further poll, and a driver that reads the
            // declaration stops polling. Ask, so that ending is not read as an
            // interruption.
            Poll::Ready(Some(Ok(_))) if this.inner.is_end_stream() => {
                this.report(Delivery::Complete);
            }
            // A frame that failed leaves the body unfinished, and the drop
            // below reports it as such. A frame with more behind it is not an
            // ending at all.
            Poll::Ready(Some(_)) | Poll::Pending => {}
        }

        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        // A body already at its end was delivered whether or not anything
        // polled it again: an empty response is the ordinary case, and a driver
        // that consults `is_end_stream` first need never call `poll_frame` at
        // all.
        let delivery = if self.inner.is_end_stream() {
            Delivery::Complete
        } else {
            Delivery::Interrupted
        };

        // Release what is wrapped before reporting: an observer told the body
        // is gone may treat whatever it held as gone too. The guard reports on
        // its own drop, so a release that panics still reports as it unwinds.
        let _report = Reporting {
            report: self.report.take(),
            delivery,
        };
        drop(mem::take(&mut self.inner));
    }
}

/// Makes a [`Watched`] body's report when dropped, including by an unwind.
struct Reporting {
    report: Option<Box<dyn FnOnce(Delivery) + Send>>,
    delivery: Delivery,
}

impl Drop for Reporting {
    fn drop(&mut self) {
        if let Some(report) = self.report.take() {
            report(self.delivery);
        }
    }
}
