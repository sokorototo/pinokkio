use std::{cell, collections, future::Future, marker, pin::Pin, sync::mpsc, task, thread, time};

thread_local! {
	/// Used by `sleep` to queue new timer futures. If a queue exists, then the thread-id of the sleeping thread is known
	static SLEEPING_THREAD: cell::RefCell<Option<(thread::Thread, mpsc::Sender<TimerTracker>)>> = cell::RefCell::new(None);
}

/// Spawns a dedicated lightweight sleeping thread for OS preemption of sleeping futures
pub fn init() {
	SLEEPING_THREAD.with_borrow_mut(|queue| {
		if let None = queue {
			// init sleeping thread and current thread state
			let (sender, receiver) = mpsc::channel::<TimerTracker>();

			// start sleeping thread
			let sleeper = thread::spawn(move || {
				let mut timers = collections::BinaryHeap::<TimerTracker>::new();
				// Timers that are overdue, but haven't been polled yet. Thus no waker is available
				let mut zombie_timers = Vec::new();

				loop {
					let now = time::Instant::now();

					// insert new timer futures
					timers.extend(receiver.try_iter());

					// pop due overdue timers from queue
					while timers.peek().map(|t| t.due <= now).unwrap_or(false) {
						if let Some(TimerTracker { waker_rx, .. }) = timers.pop() {
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

					// if we have any timers pending, sleep and wake task
					if let Some(e) = timers.peek() {
						thread::sleep(e.due - time::Instant::now());
					} else {
						// runtime thread will unpark sleeping thread to process any new timers
						thread::park();
					}
				}
			});

			*queue = Some((sleeper.thread().clone(), sender));
		}
	});
}

/// Keeps track of when a timer is due, as well as a waker to poll the adjacent future.
struct TimerTracker {
	due: time::Instant,
	waker_rx: oneshot::Receiver<task::Waker>,
}

impl PartialEq for TimerTracker {
	fn eq(&self, other: &Self) -> bool {
		self.due == other.due
	}
}

impl Eq for TimerTracker {}

impl PartialOrd for TimerTracker {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		other.due.partial_cmp(&self.due)
	}
}

impl Ord for TimerTracker {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		other.due.cmp(&self.due)
	}
}

/// Creates a new [`Sleep`] future
pub fn sleep(dur: time::Duration) -> Sleep {
	let due = time::Instant::now() + dur;
	let (sender, waker_rx) = oneshot::channel();

	SLEEPING_THREAD.with_borrow(|s| match s {
		Some((thread, sender)) => {
			sender.send(TimerTracker { due, waker_rx }).unwrap();
			// unpark sleeping thread
			thread.unpark();
		}
		None => panic!("Sleeping thread has not been initialized"),
	});

	Sleep { due, sender: Some(sender), _marker: marker::PhantomData }
}

/// Immediately returns if `due` has already passed during the time of invocation.
pub struct Sleep {
	pub(crate) due: time::Instant,
	pub(crate) sender: Option<oneshot::Sender<task::Waker>>,
	pub(crate) _marker: marker::PhantomData<*mut u8>,
}

impl Unpin for Sleep {}

impl Future for Sleep {
	type Output = time::Instant;

	fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
		// we've been woken by the runtime, as the oneshot is consumed
		if self.sender.is_none() {
			return task::Poll::Ready(self.due);
		}

		// avoid waking self if due is passed
		match time::Instant::now() > self.due {
			true => task::Poll::Ready(self.due),
			false => {
				// the runtime will wake us when timer is done
				if let Some(s) = self.sender.take() {
					s.send(cx.waker().clone()).expect("Receiver dropped, can't send Waker");
				}

				task::Poll::Pending
			}
		}
	}
}
