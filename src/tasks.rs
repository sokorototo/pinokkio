use std::{future::Future, pin::Pin, task};

/// A long running future, results can be polled using [`TaskMonitor`]
pub(crate) struct Task {
	pub(crate) inner: Pin<Box<dyn Future<Output = ()>>>,
	pub(crate) waker: task::Waker,
	pub(crate) monitor_waker: Option<oneshot::Receiver<task::Waker>>,
}

/// Future that stays pending until [`Task`](crate::rt::Runtime::spawn) completes
pub struct TaskMonitor<T> {
	pub(crate) result_rx: oneshot::Receiver<T>,
	pub(crate) waker_tx: Option<oneshot::Sender<task::Waker>>,
}

impl<T> Unpin for TaskMonitor<T> {}

impl<T> Future for TaskMonitor<T> {
	type Output = Option<T>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
		match self.result_rx.try_recv() {
			Ok(v) => task::Poll::Ready(Some(v)),
			Err(oneshot::TryRecvError::Empty) => {
				if let Some(tx) = self.waker_tx.take() {
					if tx.send(cx.waker().clone()).is_err() {
						return task::Poll::Ready(None);
					};
				}

				task::Poll::Pending
			}
			Err(oneshot::TryRecvError::Disconnected) => task::Poll::Ready(None),
		}
	}
}
