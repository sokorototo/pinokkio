#![doc = include_str!("../README.md")]

#[cfg(test)]
mod tests;

/// Runtime implementation and interface
pub mod rt;
/// Tasks and Task Monitor (Join Handles) implementation
pub mod tasks;
/// Lazy Timers implementation, focused on reducing self wake-ups
#[cfg(feature = "timers")]
pub mod timers;
