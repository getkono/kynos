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
/// The callback is held inline rather than boxed, so watching a body costs the
/// one box that erases this struct and nothing beside it.
pub(super) struct Watched<F: FnOnce(Delivery)> {
    pub(super) inner: UnsyncBoxBody<Bytes, BoxError>,
    /// `None` once the report has been made.
    pub(super) report: Option<F>,
}

// Never structurally pinned: `report` is only ever moved out whole, and
// `inner` is a box, so pinning a `Watched` pins nothing inside it.
impl<F: FnOnce(Delivery)> Unpin for Watched<F> {}

impl<F: FnOnce(Delivery)> Watched<F> {
    /// Reports `delivery`, unless something already reported.
    fn report(&mut self, delivery: Delivery) {
        if let Some(report) = self.report.take() {
            report(delivery);
        }
    }
}

impl<F: FnOnce(Delivery)> HttpBody for Watched<F> {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_frame(context);

        match &polled {
            Poll::Ready(None) => this.report(Delivery::Complete),
            // A driver may stop polling once the last frame declares the end.
            Poll::Ready(Some(Ok(_))) if this.inner.is_end_stream() => {
                this.report(Delivery::Complete);
            }
            // A failed frame is reported as interrupted on drop.
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

impl<F: FnOnce(Delivery)> Drop for Watched<F> {
    fn drop(&mut self) {
        // A body at its end was delivered even if never polled (an empty one).
        let delivery = if self.inner.is_end_stream() {
            Delivery::Complete
        } else {
            Delivery::Interrupted
        };

        // Release the inner body before reporting; the guard still reports if
        // that release panics.
        let _report = Reporting {
            report: self.report.take(),
            delivery,
        };
        drop(mem::take(&mut self.inner));
    }
}

/// Makes a [`Watched`] body's report when dropped, including by an unwind.
struct Reporting<F: FnOnce(Delivery)> {
    report: Option<F>,
    delivery: Delivery,
}

impl<F: FnOnce(Delivery)> Drop for Reporting<F> {
    fn drop(&mut self) {
        if let Some(report) = self.report.take() {
            report(self.delivery);
        }
    }
}
