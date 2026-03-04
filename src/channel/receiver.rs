//! Channel receiver implementation.

use super::metrics::ChannelMetrics;
use super::RecvError;
use crossbeam::channel::{self, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

/// The receiving half of a channel.
///
/// Receivers can be cloned to create multiple consumers for the same channel.
/// All clones share the same underlying channel and metrics.
pub struct Receiver<T> {
    inner: channel::Receiver<T>,
    metrics: Arc<ChannelMetrics>,
}

impl<T> Receiver<T> {
    /// Creates a new receiver wrapping a crossbeam receiver.
    pub(crate) fn new(inner: channel::Receiver<T>, metrics: Arc<ChannelMetrics>) -> Self {
        Self { inner, metrics }
    }

    /// Receives a value from the channel, blocking until one is available.
    ///
    /// Returns `Some(value)` on success, or `None` if the channel is disconnected.
    pub fn recv(&self) -> Option<T> {
        match self.inner.recv() {
            Ok(value) => {
                self.metrics.record_receive();
                Some(value)
            }
            Err(_) => None,
        }
    }

    /// Attempts to receive a value without blocking.
    ///
    /// Returns:
    /// - `Ok(value)` if a value was received
    /// - `Err(RecvError::Empty)` if the channel is empty
    /// - `Err(RecvError::Disconnected)` if the sender has been dropped
    pub fn try_recv(&self) -> Result<T, RecvError> {
        match self.inner.try_recv() {
            Ok(value) => {
                self.metrics.record_receive();
                Ok(value)
            }
            Err(TryRecvError::Empty) => Err(RecvError::Empty),
            Err(TryRecvError::Disconnected) => Err(RecvError::Disconnected),
        }
    }

    /// Receives a value with a timeout.
    ///
    /// Returns:
    /// - `Ok(value)` if a value was received within the timeout
    /// - `Err(RecvError::Empty)` if the timeout expired
    /// - `Err(RecvError::Disconnected)` if the sender has been dropped
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvError> {
        match self.inner.recv_timeout(timeout) {
            Ok(value) => {
                self.metrics.record_receive();
                Ok(value)
            }
            Err(RecvTimeoutError::Timeout) => Err(RecvError::Empty),
            Err(RecvTimeoutError::Disconnected) => Err(RecvError::Disconnected),
        }
    }

    /// Receives a value with a deadline.
    ///
    /// Returns:
    /// - `Ok(value)` if a value was received before the deadline
    /// - `Err(RecvError::Empty)` if the deadline passed
    /// - `Err(RecvError::Disconnected)` if the sender has been dropped
    pub fn recv_deadline(&self, deadline: std::time::Instant) -> Result<T, RecvError> {
        match self.inner.recv_deadline(deadline) {
            Ok(value) => {
                self.metrics.record_receive();
                Ok(value)
            }
            Err(RecvTimeoutError::Timeout) => Err(RecvError::Empty),
            Err(RecvTimeoutError::Disconnected) => Err(RecvError::Disconnected),
        }
    }

    /// Returns whether the channel is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns whether the channel is full.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.inner.is_full()
    }

    /// Returns the current number of items in the channel.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns the channel capacity.
    ///
    /// Returns `None` for unbounded channels.
    #[must_use]
    pub fn capacity(&self) -> Option<usize> {
        self.inner.capacity()
    }

    /// Returns a reference to the channel metrics.
    #[must_use]
    pub fn metrics(&self) -> &ChannelMetrics {
        &self.metrics
    }

    /// Creates an iterator that receives values until the channel is disconnected.
    pub fn iter(&self) -> ReceiverIter<'_, T> {
        ReceiverIter { receiver: self }
    }

    /// Creates a try iterator that attempts to receive values without blocking.
    pub fn try_iter(&self) -> TryReceiverIter<'_, T> {
        TryReceiverIter { receiver: self }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            metrics: Arc::clone(&self.metrics),
        }
    }
}

// SAFETY: Receiver is Send if T is Send
unsafe impl<T: Send> Send for Receiver<T> {}
// SAFETY: Receiver is Sync because crossbeam::Receiver is Sync
unsafe impl<T: Send> Sync for Receiver<T> {}

/// A blocking iterator over received values.
pub struct ReceiverIter<'a, T> {
    receiver: &'a Receiver<T>,
}

impl<'a, T> Iterator for ReceiverIter<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv()
    }
}

/// A non-blocking iterator over received values.
pub struct TryReceiverIter<'a, T> {
    receiver: &'a Receiver<T>,
}

impl<'a, T> Iterator for TryReceiverIter<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.try_recv().ok()
    }
}

/// An owned iterator over received values.
pub struct IntoIter<T> {
    receiver: Receiver<T>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv()
    }
}

impl<T> IntoIterator for Receiver<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter { receiver: self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::bounded;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_receiver_basic() {
        let (tx, rx) = bounded::<u32>(10);

        tx.send(1).unwrap();
        tx.send(2).unwrap();

        assert_eq!(rx.recv(), Some(1));
        assert_eq!(rx.recv(), Some(2));
    }

    #[test]
    fn test_receiver_try_recv() {
        let (tx, rx) = bounded::<u32>(10);

        // Empty channel
        assert!(matches!(rx.try_recv(), Err(RecvError::Empty)));

        tx.send(42).unwrap();
        assert_eq!(rx.try_recv(), Ok(42));
        assert!(matches!(rx.try_recv(), Err(RecvError::Empty)));
    }

    #[test]
    fn test_receiver_timeout() {
        let (tx, rx) = bounded::<u32>(10);

        // Timeout on empty channel
        let result = rx.recv_timeout(Duration::from_millis(10));
        assert!(matches!(result, Err(RecvError::Empty)));

        tx.send(42).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)), Ok(42));
    }

    #[test]
    fn test_receiver_clone() {
        let (tx, rx1) = bounded::<u32>(10);
        let rx2 = rx1.clone();

        tx.send(1).unwrap();
        tx.send(2).unwrap();

        // Both receivers can receive from the same channel
        // (they compete for messages)
        let r1 = rx1.try_recv().ok();
        let r2 = rx2.try_recv().ok();

        // One of them should get each message
        assert!(r1.is_some() || r2.is_some());
    }

    #[test]
    fn test_receiver_metrics() {
        let (tx, rx) = bounded::<u32>(10);

        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();

        rx.recv();
        rx.recv();

        assert_eq!(rx.metrics().received(), 2);
    }

    #[test]
    fn test_receiver_iter() {
        let (tx, rx) = bounded::<u32>(10);

        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();
        drop(tx); // Close the channel

        let values: Vec<_> = rx.into_iter().collect();
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn test_receiver_try_iter() {
        let (tx, rx) = bounded::<u32>(10);

        tx.send(1).unwrap();
        tx.send(2).unwrap();

        let values: Vec<_> = rx.try_iter().collect();
        assert_eq!(values, vec![1, 2]);

        // Try iter should stop when empty
        let values: Vec<_> = rx.try_iter().collect();
        assert!(values.is_empty());
    }

    #[test]
    fn test_receiver_disconnected() {
        let (tx, rx) = bounded::<u32>(10);
        drop(tx);

        assert_eq!(rx.recv(), None);
        assert!(matches!(rx.try_recv(), Err(RecvError::Disconnected)));
    }

    #[test]
    fn test_receiver_threaded() {
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
                match rx.recv() {
                    Some(v) => {
                        assert_eq!(v, expected);
                        received += 1;
                    }
                    None => panic!("channel disconnected unexpectedly"),
                }
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();
        assert_eq!(received, count);
    }
}
