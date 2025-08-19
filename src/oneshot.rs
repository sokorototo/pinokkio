use alloc::boxed::Box;
use core::{fmt, mem};

pub(crate) fn channel<T>() -> (Sender<T>, Receiver<T>) {
	let status = Box::into_raw(Box::new(ChannelStatus::Pending));

	let sender = Sender { status };
	let receiver = Receiver { status };

	(sender, receiver)
}

#[repr(u8)]
/// The status of a channel
pub(crate) enum ChannelStatus<T> {
	/// [`Sender`] is pending to send messages
	Pending,
	/// Either [`Sender`] or [`Receiver`] has been dropped, without a message passing
	Closed,
	/// A message has been currently sent by the [`Sender`]
	Active(T),
	/// A message was written to, and consumed from this channel
	Consumed,
}

impl<T> fmt::Debug for ChannelStatus<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Pending => write!(f, "Pending"),
			Self::Closed => write!(f, "Closed"),
			Self::Active(..) => f.debug_tuple("Active").field(&"[packet]").finish(),
			Self::Consumed => write!(f, "Consumed"),
		}
	}
}

pub(crate) struct Sender<T> {
	status: *mut ChannelStatus<T>,
}

impl<T> Sender<T> {
	/// If `Some(T)` then the receiver was closed, [`None`] is the success path
	pub(crate) fn send(self, data: T) -> Result<(), T> {
		let status = unsafe { self.status.as_mut().unwrap() };

		// attempt to write data to pointer
		match status {
			ChannelStatus::Pending => {
				// set status to active
				*status = ChannelStatus::Active(data);
			}
			// receiver was closed
			ChannelStatus::Closed => return Err(data),
			// double send?
			ChannelStatus::Consumed | ChannelStatus::Active(..) => unreachable!("Double Send on oneshot channel"),
		};

		Ok(())
	}
}

impl<T> Drop for Sender<T> {
	fn drop(&mut self) {
		let status = unsafe { self.status.as_mut().unwrap() };

		match status {
			// sender dropped without sending a message
			ChannelStatus::Pending => *status = ChannelStatus::Closed,
			// message already sent, or receiver dropped
			ChannelStatus::Consumed | ChannelStatus::Active(..) | ChannelStatus::Closed => {}
		}
	}
}

pub(crate) struct Receiver<T> {
	status: *mut ChannelStatus<T>,
}

impl<T> Receiver<T> {
	pub(crate) fn try_recv(&self) -> Result<T, TryRecvError> {
		let status = unsafe { self.status.as_mut().unwrap() };

		match status {
			ChannelStatus::Active(..) => {
				let ChannelStatus::Active(data) = mem::replace(status, ChannelStatus::Consumed) else { unreachable!() };
				Ok(data)
			}
			ChannelStatus::Pending => Err(TryRecvError::Empty),
			ChannelStatus::Consumed | ChannelStatus::Closed => Err(TryRecvError::Disconnected),
		}
	}
}

impl<T> Drop for Receiver<T> {
	fn drop(&mut self) {
		let _ = unsafe { Box::from_raw(self.status) };

		// update status
		let status = unsafe { self.status.as_mut().unwrap() };

		match status {
			// receiver dropped without receiving a message
			ChannelStatus::Pending => *status = ChannelStatus::Closed,
			// message already sent, or sender dropped
			ChannelStatus::Consumed | ChannelStatus::Active(..) | ChannelStatus::Closed => {}
		}
	}
}

/// Error type for [`Receiver::try_recv`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TryRecvError {
	/// Sender hasn't sent any data yet
	Empty,
	/// Sender was dropped
	Disconnected,
}

impl fmt::Display for TryRecvError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let msg = match self {
			TryRecvError::Empty => "no messages sent from sender",
			TryRecvError::Disconnected => "sender was dropped",
		};

		fmt::Display::fmt(msg, f)
	}
}
