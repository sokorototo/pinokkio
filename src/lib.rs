// TODO: wasm compatibility: promises instead of parked threads, wasm-time and set_timeout instead of sleep

#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

#[cfg(test)]
mod tests;

/// [`Runtime`](rt::Runtime) implementation
pub mod rt;
/// [`TaskMonitor`](tasks::TaskMonitor) implementation
pub mod tasks;

/// Lazy Timers implementation, focused on reducing self wake-ups
#[cfg(feature = "timers")]
pub mod timers;
