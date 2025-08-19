use super::*;

use alloc::{boxed::Box, collections, vec::Vec};
use core::{cell, future::Future, mem, task};

/// A minimal single-threaded async runtime
#[derive(Default)]
pub struct Runtime {
	/// Stores tasks to be polled when woken
	tasks: collections::BTreeMap<usize, tasks::Task>,

	/// queue of tasks woken by various wakers
	queue: cell::RefCell<Vec<usize>>,

	/// currently processing tasks: double-buffered with `queue` during `poll`
	woken: Vec<usize>,
}

impl Runtime {
	/// Instantiate a new Runtime
	pub fn new() -> Self {
		let queue = cell::RefCell::new(Vec::new());
		let woken = Vec::new();

		Self { queue, woken, tasks: collections::BTreeMap::new() }
	}

	/// Blocks execution, continuously polling tasks and waiting for `fut` to complete
	pub fn block_on<T: 'static, F: Future<Output = T> + 'static>(&mut self, fut: F) -> T {
		let task_id = self.tasks.len();
		let (results_tx, results_rx) = oneshot::channel();

		let waker = self.insert_task(task_id);
		waker.wake_by_ref(); // poll once

		let inner = Box::pin(async move {
			let res = fut.await;

			if let Err(_) = results_tx.send(res) {
				panic!("Unable to send results for completed task: {}", task_id)
			};
		});

		self.tasks.insert(task_id, tasks::Task { inner, waker, monitor_waker: None });

		loop {
			self.poll();

			match results_rx.try_recv() {
				Ok(r) => return r,
				Err(oneshot::TryRecvError::Empty) => {}
				Err(oneshot::TryRecvError::Disconnected) => unreachable!("Task was dropped during execution"),
			}
		}
	}

	/// Spawns a future as a [`Task`](tasks::Task), and returns a [`TaskMonitor`](tasks::TaskMonitor)
	pub fn spawn<T: 'static, F: Future<Output = T> + 'static>(&mut self, fut: F) -> tasks::TaskMonitor<T> {
		let task_id = self.tasks.len();
		let (result_tx, result_rx) = oneshot::channel();
		let (waker_tx, waker_rx) = oneshot::channel();

		// poll once, and initialize task
		let waker = self.insert_task(task_id);
		waker.wake_by_ref();

		let inner = Box::pin(async move {
			let res = fut.await;

			if let Err(_) = result_tx.send(res) {
				panic!("Unable to send results for completed task: {}", task_id)
			};
		});

		let task = tasks::Task { inner, waker, monitor_waker: Some(waker_rx) };
		self.tasks.insert(task_id, task);

		tasks::TaskMonitor { result_rx, waker_tx: Some(waker_tx) }
	}

	fn insert_task(&mut self, id: usize) -> task::Waker {
		static WAKER_VTABLE: task::RawWakerVTable = task::RawWakerVTable::new(clone, wake, wake_by_ref, drop);
		type WakerData = (*const cell::RefCell<Vec<usize>>, usize);

		// quartet of waker methods
		unsafe fn clone(data: *const ()) -> task::RawWaker {
			let data = data as *const WakerData;
			let data = unsafe { data.as_ref() }.expect("Got NULL as waker data");

			// create a new clone to avoid a double-free
			let clone = Box::leak(Box::new(*data));
			task::RawWaker::new(clone as *const WakerData as *const (), &WAKER_VTABLE)
		}

		unsafe fn wake(data: *const ()) {
			unsafe {
				wake_by_ref(data);
				drop(data);
			}
		}

		unsafe fn wake_by_ref(data: *const ()) {
			let data = data as *const WakerData;
			let data = unsafe { data.as_ref() }.expect("Got NULL as waker data");

			let (queue, id) = data;
			if let Some(queue) = unsafe { queue.as_ref() } {
				let mut queue = queue.borrow_mut();
				queue.push(*id);
			}
		}

		unsafe fn drop(data: *const ()) {
			let data = data as *const WakerData as *mut WakerData;
			let data = unsafe { data.as_mut() }.expect("Got NULL as waker data");

			unsafe {
				let data: Box<WakerData> = Box::from_raw(data);
				mem::drop(data);
			}
		}

		let data: WakerData = (&self.queue as *const _, id);
		let data = Box::leak(Box::new(data));

		// simple waker that adds id to vector
		unsafe { task::Waker::new(data as *const WakerData as *const (), &WAKER_VTABLE) }
	}

	/// must be called manually to progress execution of tasks and timers
	fn poll(&mut self) {
		// swap buffers
		let mut queue = self.queue.borrow_mut();
		mem::swap(&mut self.woken, &mut queue);
		mem::drop(queue);

		// poll queued tasks
		for next in self.woken.drain(..) {
			let mut remove = false;

			if let Some(task) = self.tasks.get_mut(&next) {
				let fut = task.inner.as_mut();
				let mut context = task::Context::from_waker(&task.waker);

				if let task::Poll::Ready(_) = fut.poll(&mut context) {
					if let Some(waker_rx) = task.monitor_waker.take() {
						if let Ok(waker) = waker_rx.try_recv() {
							waker.wake()
						}
					}

					remove = true;
				}
			}

			if remove {
				self.tasks.remove(&next);
			}
		}
	}
}
