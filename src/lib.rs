use std::{collections::BTreeMap, future::Future, pin::Pin, sync::mpsc, task, time};
use waker_fn::waker_fn;

#[cfg(test)]
mod tests;

pub mod tasks;
pub mod timers;

pub struct Task {
	inner: Pin<Box<dyn Future<Output = ()>>>,
	waker: task::Waker,
	monitor_waker: Option<oneshot::Receiver<task::Waker>>,
}

pub struct Runtime {
	tasks: BTreeMap<usize, Task>,
	queue_tx: mpsc::Sender<usize>,
	queue_rx: mpsc::Receiver<usize>,
}

impl Runtime {
	pub fn new() -> Self {
		let (queue, next) = mpsc::channel();
		Self { tasks: BTreeMap::new(), queue_tx: queue, queue_rx: next }
	}

	pub fn block_on<T: 'static, F: Future<Output = T> + 'static>(&mut self, fut: F) -> Option<T> {
		let task_id = self.tasks.len();
		let (results_tx, results_rx) = oneshot::channel();

		let waker = waker_fn({
			let queue = self.queue_tx.clone();
			move || queue.send(task_id).unwrap()
		});

		let inner = Box::pin(async move {
			let res = fut.await;
			results_tx.send(res).unwrap();
		});

		waker.wake_by_ref(); // poll once
		self.tasks.insert(task_id, Task { inner, waker, monitor_waker: None });

		loop {
			self.poll();

			match results_rx.try_recv() {
				Ok(r) => return Some(r),
				Err(oneshot::TryRecvError::Disconnected) => return None,
				Err(oneshot::TryRecvError::Empty) => {}
			}
		}
	}

	pub fn spawn<T: 'static, F: Future<Output = T> + 'static>(&mut self, fut: F) -> tasks::TaskMonitor<T> {
		let task_id = self.tasks.len();
		let (result_tx, result_rx) = oneshot::channel();
		let (waker_tx, waker_rx) = oneshot::channel();

		// setup auto task
		let waker = waker_fn({
			let queue = self.queue_tx.clone();
			move || queue.send(task_id).unwrap()
		});

		let inner = Box::pin(async move {
			let res = fut.await;
			result_tx.send(res).unwrap();
		});

		// poll once, and add to list of tasks
		waker.wake_by_ref();
		let task = Task { inner, waker, monitor_waker: Some(waker_rx) };
		self.tasks.insert(task_id, task);

		tasks::TaskMonitor { result_rx, waker_tx: Some(waker_tx) }
	}

	pub fn poll(&mut self) {
		// poll timers
		let mut timers = timers::TIMERS.lock().unwrap();
		let mut zombie_timers = timers::ZOMBIE_TIMERS.lock().unwrap();

		let now = time::Instant::now();
		let mut idx = None;

		for (i, (due, ..)) in timers.iter_mut().enumerate() {
			if now >= *due {
				let _ = idx.insert(i);
				break;
			}
		}

		if let Some(idx) = idx {
			for (_, waker_rx) in timers.drain(idx..) {
				match waker_rx.try_recv() {
					Ok(waker) => waker.wake(),
					// timer is due, but hasn't been polled yet
					Err(oneshot::TryRecvError::Empty) => zombie_timers.push(waker_rx),
					// timer is due, but was dropped. either dropped itself or dropped prematurely
					Err(oneshot::TryRecvError::Disconnected) => (),
				}
			}
		}

		// attempt to poll zombie timers
		let mut old_zombies = Vec::with_capacity(zombie_timers.len());
		for waker_rx in zombie_timers.drain(..) {
			match waker_rx.try_recv() {
				Ok(waker) => waker.wake(),
				Err(oneshot::TryRecvError::Empty) => old_zombies.push(waker_rx),
				Err(oneshot::TryRecvError::Disconnected) => (),
			}
		}

		zombie_timers.append(&mut old_zombies);
		drop((zombie_timers, timers)); // release locks

		// poll next task
		if let Ok(next) = self.queue_rx.try_recv() {
			if let Some(mut task) = self.tasks.remove(&next) {
				let fut = task.inner.as_mut();
				let mut context = task::Context::from_waker(&task.waker);

				match fut.poll(&mut context) {
					task::Poll::Pending => {
						self.tasks.insert(next, task);
					}
					// attempt to wake-up monitor
					task::Poll::Ready(_) => {
						if let Some(waker_rx) = task.monitor_waker.take() {
							match waker_rx.try_recv() {
								Ok(waker) => waker.wake(),
								Err(oneshot::TryRecvError::Empty) => {
									panic!("Attempted to wake up a completed task's monitor")
								}
								Err(oneshot::TryRecvError::Disconnected) => (),
							}
						}
					}
				}
			};
		}
	}
}
