use std::{future::Future, pin::Pin, sync, task, time};

/// Timers yet overdue, dropping the future clears the timer
pub static TIMERS: sync::Mutex<Vec<(time::Instant, oneshot::Receiver<task::Waker>)>> = sync::Mutex::new(Vec::new());

/// Timers that are overdue, but haven't been polled yet. Thus no waker is available
pub(crate) static ZOMBIE_TIMERS: sync::Mutex<Vec<oneshot::Receiver<task::Waker>>> = sync::Mutex::new(Vec::new());

/// Long running future that progresses execution of [`Sleep`] futures, must be spawned for [`Sleep`] to work
pub struct SleepSubroutine;

impl Unpin for SleepSubroutine {}

impl Future for SleepSubroutine {
	type Output = ();

	fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
		poll();
		cx.waker().wake_by_ref();

		task::Poll::Pending
	}
}

/// Actually progresses the execution of stored futures
pub(crate) fn poll() {
	let mut timers = TIMERS.lock().unwrap();
	let mut zombie_timers = ZOMBIE_TIMERS.lock().unwrap();

	let now = time::Instant::now();
	let mut cutoff = None;

	for (i, (due, ..)) in timers.iter_mut().enumerate() {
		if now >= *due {
			cutoff = Some(i);
			break;
		}
	}

	if let Some(idx) = cutoff {
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
}

/// Creates a new [`Sleep`] future
pub fn sleep(dur: time::Duration) -> Sleep {
	let due = time::Instant::now() + dur;

	let (sender, waker_rx) = oneshot::channel();
	let mut timers = TIMERS.lock().unwrap();

	// sorted by due time in descending order
	let idx = timers.iter().enumerate().find(|(_, (d, ..))| *d < due).map(|(i, ..)| i);

	match idx {
		Some(i) => timers.insert(i, (due, waker_rx)),
		// empty vector, so insert at front
		None => timers.push((due, waker_rx)),
	}

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
