//! Lock-free channel implementations for inter-stage communication.
//!
//! This module provides high-performance, lock-free queues for communication
//! between pipeline stages. These are critical for real-time systems where
//! lock contention can cause unpredictable latency spikes.
//!
//! # Channel Types
//!
//! - [`BoundedChannel`]: Fixed-size channel with backpressure
//! - [`UnboundedChannel`]: Unbounded channel for best-effort stages
//! - [`StageChannel`]: Wrapper with metrics and backpressure handling
//!
//! # Example
//!
//! ```rust
//! use sensor_pipeline::channel::{bounded, StageChannel};
//!
//! let (tx, rx) = bounded::<u32>(1024);
//! tx.send(42).unwrap();
//! assert_eq!(rx.recv(), Some(42));
//! ```

mod metrics;
mod sender;
mod receiver;

pub use metrics::ChannelMetrics;
pub use sender::Sender;
pub use receiver::Receiver;

use std::sync::Arc;
use crossbeam::channel::{self};

/// Creates a bounded channel with the specified capacity.
///
/// The channel will block producers when full (or return an error on try_send).
/// This is the recommended channel type for most pipeline stages as it provides
/// natural backpressure.
///
/// # Example
///
/// ```rust
/// use sensor_pipeline::channel::bounded;
///
/// let (tx, rx) = bounded::<i32>(100);
/// tx.send(1).unwrap();
/// assert_eq!(rx.recv(), Some(1));
/// ```
#[must_use]
pub fn bounded<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = channel::bounded(capacity);
    let metrics = Arc::new(ChannelMetrics::new(capacity));

    (
        Sender::new(tx, Arc::clone(&metrics)),
        Receiver::new(rx, metrics),
    )
}

/// Creates an unbounded channel.
///
/// Use with caution - unbounded channels can grow without limit if the consumer
/// is slower than the producer. Best for non-critical stages like metrics collection.
///
/// # Example
///
/// ```rust
/// use sensor_pipeline::channel::unbounded;
///
/// let (tx, rx) = unbounded::<i32>();
/// tx.send(1).unwrap();
/// assert_eq!(rx.recv(), Some(1));
/// ```
#[must_use]
pub fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = channel::unbounded();
    let metrics = Arc::new(ChannelMetrics::new(usize::MAX));

    (
        Sender::new(tx, Arc::clone(&metrics)),
        Receiver::new(rx, metrics),
    )
}

/// A wrapper around a channel that tracks metrics and handles backpressure.
///
/// This is the primary interface for inter-stage communication in the pipeline.
pub struct StageChannel<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
    name: String,
}

impl<T> StageChannel<T> {
    /// Creates a new stage channel with the given name and capacity.
    #[must_use]
    pub fn new(name: impl Into<String>, capacity: usize) -> Self {
        let (sender, receiver) = bounded(capacity);
        Self {
            sender,
            receiver,
            name: name.into(),
        }
    }

    /// Returns the channel name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the sender.
    #[must_use]
    pub fn sender(&self) -> &Sender<T> {
        &self.sender
    }

    /// Returns a reference to the receiver.
    #[must_use]
    pub fn receiver(&self) -> &Receiver<T> {
        &self.receiver
    }

    /// Splits the channel into sender and receiver.
    pub fn split(self) -> (Sender<T>, Receiver<T>) {
        (self.sender, self.receiver)
    }

    /// Returns the current metrics for this channel.
    #[must_use]
    pub fn metrics(&self) -> &ChannelMetrics {
        self.sender.metrics()
    }
}

/// Result type for send operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError<T> {
    /// The channel is full.
    Full(T),
    /// The channel is disconnected.
    Disconnected(T),
}

impl<T> SendError<T> {
    /// Returns the unsent value.
    pub fn into_inner(self) -> T {
        match self {
            SendError::Full(v) | SendError::Disconnected(v) => v,
        }
    }
}

/// Result type for receive operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvError {
    /// The channel is empty.
    Empty,
    /// The channel is disconnected.
    Disconnected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    

    #[test]
    fn test_bounded_channel_basic() {
        let (tx, rx) = bounded::<u32>(10);

        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();

        assert_eq!(rx.recv(), Some(1));
        assert_eq!(rx.recv(), Some(2));
        assert_eq!(rx.recv(), Some(3));
    }

    #[test]
    fn test_bounded_channel_full() {
        let (tx, rx) = bounded::<u32>(2);

        tx.send(1).unwrap();
        tx.send(2).unwrap();

        // Channel should be full now
        assert!(tx.try_send(3).is_err());

        // Drain one and try again
        rx.recv();
        assert!(tx.try_send(3).is_ok());
    }

    #[test]
    fn test_unbounded_channel() {
        let (tx, rx) = unbounded::<u32>();

        for i in 0..1000 {
            tx.send(i).unwrap();
        }

        for i in 0..1000 {
            assert_eq!(rx.recv(), Some(i));
        }
    }

    #[test]
    fn test_channel_metrics() {
        let (tx, rx) = bounded::<u32>(100);

        for i in 0..50 {
            tx.send(i).unwrap();
        }

        let metrics = tx.metrics();
        assert_eq!(metrics.sent(), 50);
        assert!(metrics.current_depth() <= 50);

        for _ in 0..50 {
            rx.recv();
        }

        assert_eq!(metrics.received(), 50);
    }

    #[test]
    fn test_channel_threaded() {
        let (tx, rx) = bounded::<u32>(1024);
        let count = 10_000u32;

        let producer = thread::spawn(move || {
            for i in 0..count {
                tx.send(i).unwrap();
            }
        });

        let consumer = thread::spawn(move || {
            let mut received = 0u32;
            for expected in 0..count {
                loop {
                    match rx.recv() {
                        Some(v) => {
                            assert_eq!(v, expected);
                            received += 1;
                            break;
                        }
                        None => thread::yield_now(),
                    }
                }
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();
        assert_eq!(received, count);
    }

    #[test]
    fn test_stage_channel() {
        let channel = StageChannel::<u32>::new("test_stage", 100);

        assert_eq!(channel.name(), "test_stage");

        channel.sender().send(42).unwrap();
        assert_eq!(channel.receiver().recv(), Some(42));
    }
}
