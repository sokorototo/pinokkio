#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod oneshot;
#[cfg(test)]
mod tests;

/// [`Runtime`](rt::Runtime) implementation
pub mod rt;
/// [`Tasks`](tasks::Task) and [`TaskMonitor`](tasks::TaskMonitor) (Join Handles) implementation
pub mod tasks;
/// Lazy Timers implementation, focused on reducing self wake-ups
#[cfg(feature = "timers")]
pub mod timers;
