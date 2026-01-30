//! Channel sender implementation.

use super::metrics::ChannelMetrics;
use super::SendError;
use crossbeam::channel::{self, TrySendError};
use std::sync::Arc;
use std::time::Duration;

/// The sending half of a channel.
///
/// Senders can be cloned to create multiple producers for the same channel.
/// All clones share the same underlying channel and metrics.
pub struct Sender<T> {
    inner: channel::Sender<T>,
    metrics: Arc<ChannelMetrics>,
}

impl<T> Sender<T> {
    /// Creates a new sender wrapping a crossbeam sender.
    pub(crate) fn new(inner: channel::Sender<T>, metrics: Arc<ChannelMetrics>) -> Self {
        Self { inner, metrics }
    }

    /// Sends a value into the channel, blocking if necessary.
    ///
    /// Returns `Ok(())` on success, or `Err(SendError::Disconnected)` if the
    /// receiver has been dropped.
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        match self.inner.send(value) {
            Ok(()) => {
                self.metrics.record_send();
                Ok(())
            }
            Err(e) => Err(SendError::Disconnected(e.into_inner())),
        }
    }

    /// Attempts to send a value without blocking.
    ///
    /// Returns:
    /// - `Ok(())` on success
    /// - `Err(SendError::Full(value))` if the channel is full
    /// - `Err(SendError::Disconnected(value))` if the receiver has been dropped
    pub fn try_send(&self, value: T) -> Result<(), SendError<T>> {
        match self.inner.try_send(value) {
            Ok(()) => {
                self.metrics.record_send();
                Ok(())
            }
            Err(TrySendError::Full(v)) => {
                self.metrics.record_drop();
                Err(SendError::Full(v))
            }
            Err(TrySendError::Disconnected(v)) => Err(SendError::Disconnected(v)),
        }
    }

    /// Attempts to send a value with a timeout.
    ///
    /// Returns:
    /// - `Ok(())` on success
    /// - `Err(SendError::Full(value))` if the timeout expired
    /// - `Err(SendError::Disconnected(value))` if the receiver has been dropped
    pub fn send_timeout(&self, value: T, timeout: Duration) -> Result<(), SendError<T>> {
        match self.inner.send_timeout(value, timeout) {
            Ok(()) => {
                self.metrics.record_send();
                Ok(())
            }
            Err(channel::SendTimeoutError::Timeout(v)) => {
                self.metrics.record_drop();
                Err(SendError::Full(v))
            }
            Err(channel::SendTimeoutError::Disconnected(v)) => Err(SendError::Disconnected(v)),
        }
    }

    /// Returns whether the channel is full.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.inner.is_full()
    }

    /// Returns whether the channel is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
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

    /// Sends a value, dropping it if the channel is full.
    ///
    /// Returns `true` if the value was sent, `false` if it was dropped.
    pub fn send_or_drop(&self, value: T) -> bool {
        match self.try_send(value) {
            Ok(()) => true,
            Err(_) => false,
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            metrics: Arc::clone(&self.metrics),
        }
    }
}

// SAFETY: Sender is Send if T is Send
unsafe impl<T: Send> Send for Sender<T> {}
// SAFETY: Sender is Sync because crossbeam::Sender is Sync
unsafe impl<T: Send> Sync for Sender<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::bounded;

    #[test]
    fn test_sender_basic() {
        let (tx, _rx) = bounded::<u32>(10);

        assert!(tx.send(1).is_ok());
        assert!(tx.send(2).is_ok());

        assert_eq!(tx.len(), 2);
        assert!(!tx.is_empty());
        assert!(!tx.is_full());
    }

    #[test]
    fn test_sender_full() {
        let (tx, _rx) = bounded::<u32>(2);

        assert!(tx.send(1).is_ok());
        assert!(tx.send(2).is_ok());
        assert!(tx.is_full());

        // try_send should fail when full
        let result = tx.try_send(3);
        assert!(matches!(result, Err(SendError::Full(3))));
    }

    #[test]
    fn test_sender_clone() {
        let (tx1, rx) = bounded::<u32>(10);
        let tx2 = tx1.clone();

        tx1.send(1).unwrap();
        tx2.send(2).unwrap();

        assert_eq!(rx.recv(), Some(1));
        assert_eq!(rx.recv(), Some(2));

        // Both clones share metrics
        assert_eq!(tx1.metrics().sent(), 2);
        assert_eq!(tx2.metrics().sent(), 2);
    }

    #[test]
    fn test_send_or_drop() {
        let (tx, _rx) = bounded::<u32>(2);

        assert!(tx.send_or_drop(1));
        assert!(tx.send_or_drop(2));
        assert!(!tx.send_or_drop(3)); // Should be dropped
        assert!(!tx.send_or_drop(4)); // Should be dropped

        assert_eq!(tx.metrics().dropped(), 2);
    }

    #[test]
    fn test_send_timeout() {
        let (tx, _rx) = bounded::<u32>(1);

        tx.send(1).unwrap();

        let result = tx.send_timeout(2, Duration::from_millis(10));
        assert!(matches!(result, Err(SendError::Full(2))));
    }
}
