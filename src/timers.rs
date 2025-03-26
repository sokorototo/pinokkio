use std::{future::Future, pin::Pin, sync, task, time};

/// Timers yet overdue, dropping the future clears the timer
pub static TIMERS: sync::Mutex<Vec<(time::Instant, oneshot::Receiver<task::Waker>)>> = sync::Mutex::new(Vec::new());

/// Timers that are overdue, but haven't been polled yet. Thus no waker is available
pub(crate) static ZOMBIE_TIMERS: sync::Mutex<Vec<oneshot::Receiver<task::Waker>>> = sync::Mutex::new(Vec::new());

pub fn sleep(dur: time::Duration) -> Sleep {
	let due = time::Instant::now() + dur;

	let (sender, waker_rx) = oneshot::channel();
	let mut timers = TIMERS.lock().unwrap();

	// sorted by due time in descending order
	let idx = timers.iter().enumerate().find(|(_, (d, ..))| *d < due).map(|(i, ..)| i);

	match idx {
		Some(i) => timers.insert(i, (due.clone(), waker_rx)),
		// empty vector, so insert at front
		None => timers.push((due.clone(), waker_rx)),
	}

	Sleep { due, sender: Some(sender) }
}

/// A Sleeping future that is woken manually by the runtime, and doesn't poll itself
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
			return task::Poll::Ready(self.due.clone());
		}

		// avoid waking self if due is passed
		match time::Instant::now() > self.due {
			true => task::Poll::Ready(self.due.clone()),
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
