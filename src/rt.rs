use super::*;

use std::{collections, future::Future, sync::mpsc, task, time};
use waker_fn::waker_fn;

pub struct Runtime {
	/// Stores tasks to be polled when woken
	tasks: collections::BTreeMap<usize, tasks::Task>,

	/// Used by Wakers to queue a task to be polled in `Runtime::poll`
	task_tx: mpsc::Sender<usize>,
	/// Polls the next task
	task_rx: mpsc::Receiver<usize>,
}

impl Runtime {
	pub fn new() -> Self {
		let (task_tx, task_rx) = mpsc::channel();
		Self { task_tx, task_rx, tasks: collections::BTreeMap::new() }
	}

	pub fn block_on<T: 'static, F: Future<Output = T> + 'static>(&mut self, fut: F) -> T {
		let task_id = self.tasks.len();
		let (results_tx, results_rx) = oneshot::channel();

		let waker = self.insert_task(task_id);
		waker.wake_by_ref(); // poll once

		let inner = Box::pin(async move {
			let res = fut.await;
			results_tx.send(res).unwrap();
		});

		self.tasks.insert(task_id, tasks::Task { inner, waker, monitor_waker: None });

		loop {
			self.poll();

			match results_rx.try_recv() {
				Ok(r) => return r,
				Err(oneshot::TryRecvError::Empty) => {}
				Err(oneshot::TryRecvError::Disconnected) => panic!("Task was dropped during execution"),
			}
		}
	}

	pub fn spawn<T: 'static, F: Future<Output = T> + 'static>(&mut self, fut: F) -> tasks::TaskMonitor<T> {
		let task_id = self.tasks.len();
		let (result_tx, result_rx) = oneshot::channel();
		let (waker_tx, waker_rx) = oneshot::channel();

		// poll once, and initialize task
		let waker = self.insert_task(task_id);
		waker.wake_by_ref();

		let inner = Box::pin(async move {
			let res = fut.await;
			result_tx.send(res).unwrap();
		});

		let task = tasks::Task { inner, waker, monitor_waker: Some(waker_rx) };
		self.tasks.insert(task_id, task);

		tasks::TaskMonitor { result_rx, waker_tx: Some(waker_tx) }
	}

	fn insert_task(&mut self, id: usize) -> task::Waker {
		waker_fn({
			let tx = self.task_tx.clone();
			move || tx.send(id).unwrap()
		})
	}

	fn poll(&mut self) {
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
		if let Ok(next) = self.task_rx.try_recv() {
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
							if let Ok(waker) = waker_rx.try_recv() {
								waker.wake()
							}
						}
					}
				}
			};
		}
	}
}
