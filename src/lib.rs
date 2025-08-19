#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

extern crate alloc;

mod oneshot;
#[cfg(test)]
mod tests;

/// [`Runtime`](rt::Runtime) implementation
pub mod rt;
/// [`Tasks`](tasks::Task) and [`TaskMonitor`](tasks::TaskMonitor) (Join Handles) implementation
pub mod tasks;

/// Lazy Timers implementation, focused on reducing self wake-ups
#[cfg(all(feature = "timers", feature = "std"))]
pub mod timers;
