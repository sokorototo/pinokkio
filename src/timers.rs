// TODO: wasm compatibility: wasm-time and set_timeout instead of sleep

use crate::oneshot;
use std::{cell, collections, future::Future, pin::Pin, task, thread, time};

thread_local! {
	/// Waker used to wake timer subroutine when no sleep tasks are currently pending
	static WAKER: cell::RefCell<Option<task::Waker>> = cell::RefCell::new(None);

	/// Timers yet overdue, dropping the future clears the timer
	static TIMERS: cell::RefCell<collections::BinaryHeap<TimerTracker>> = cell::RefCell::new(collections::BinaryHeap::new());

	/// Timers that are overdue, but haven't been polled yet. Thus no waker is available
	static ZOMBIE_TIMERS: cell::RefCell<Vec<oneshot::Receiver<task::Waker>>> = cell::RefCell::new(Vec::new());
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

/// Long running future that progresses execution of [`Sleep`] futures, must be spawned for [`Sleep`] to work
pub struct SleepSubroutine;

impl Unpin for SleepSubroutine {}

impl Future for SleepSubroutine {
	type Output = ();

	fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
		// init waker if not initialized
		WAKER.with_borrow_mut(|w| {
			w.get_or_insert(cx.waker().clone());
		});

		// do our job as a sleep subroutine
		let (zombies, earliest) = TIMERS.with_borrow_mut(|timers| {
			let zombies = ZOMBIE_TIMERS.with_borrow_mut(|zombie_timers| {
				let now = time::Instant::now();

				// pop due timers from queue
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
				zombie_timers.len()
			});

			(zombies, timers.peek().map(|t| t.due.clone()))
		});

		if zombies != 0 {
			// busy loop, waiting for any zombies to resurrect
			cx.waker().wake_by_ref()
		} else {
			// if we have any timers pending, sleep and wake task
			if let Some(e) = earliest {
				thread::sleep(e - time::Instant::now());
				cx.waker().wake_by_ref()
			}
		}

		task::Poll::Pending
	}
}

/// Creates a new [`Sleep`] future
pub fn sleep(dur: time::Duration) -> Sleep {
	let due = time::Instant::now() + dur;

	let (sender, waker_rx) = oneshot::channel();

	TIMERS.with_borrow_mut(|t| t.push(TimerTracker { due, waker_rx }));
	WAKER.with_borrow(|w| w.as_ref().map(|w| w.wake_by_ref()));

	Sleep { due, sender: Some(sender) }
}

/// A sleeping future that doesn't poll itself, but [`poll`] must be called to progress execution.
/// Immediately returns if `due` has already passed during the time of invocation.
pub struct Sleep {
	pub(crate) due: time::Instant,
	pub(crate) sender: Option<oneshot::Sender<task::Waker>>,
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
