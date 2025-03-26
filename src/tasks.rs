use std::{future::Future, pin::Pin, task};

// TODO: Allow killing the auto-task
pub struct TaskMonitor<T> {
	pub(crate) result_rx: oneshot::Receiver<T>,
	pub(crate) waker_tx: Option<oneshot::Sender<task::Waker>>,
}

impl<T> Unpin for TaskMonitor<T> {}

impl<T> Future for TaskMonitor<T> {
	type Output = T;

	fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
		match self.result_rx.try_recv() {
			Ok(v) => task::Poll::Ready(v),
			Err(oneshot::TryRecvError::Empty) => {
				if let Some(tx) = self.waker_tx.take() {
					tx.send(cx.waker().clone()).expect("Task was dropped before TaskMonitor was notified");
				}

				task::Poll::Pending
			}
			Err(oneshot::TryRecvError::Disconnected) => panic!("TaskMonitor disconnected"),
		}
	}
}
