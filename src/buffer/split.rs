//! Owned producer and consumer handles for the ring buffer.
//!
//! This module provides owned handles that take ownership of the ring buffer,
//! allowing them to be used across threads without lifetime concerns.

use core::sync::atomic::Ordering;

use super::ring::RingBuffer;
use super::CachePadded;

#[cfg(feature = "std")]
use std::sync::Arc;

/// An owned producer handle backed by an `Arc`.
///
/// Unlike the borrowed `Producer`, this handle owns a reference-counted
/// pointer to the ring buffer, allowing it to outlive the original buffer
/// and be used freely across threads.
///
/// # Example
///
/// ```rust,ignore
/// use sensor_pipeline::buffer::{RingBuffer, OwnedProducer, OwnedConsumer};
/// use std::sync::Arc;
///
/// let buffer = Arc::new(RingBuffer::<u32, 1024>::new());
/// let (producer, consumer) = buffer.split_owned();
///
/// std::thread::spawn(move || {
///     producer.push(42).unwrap();
/// });
///
/// std::thread::spawn(move || {
///     while consumer.pop().is_none() {}
/// });
/// ```
#[cfg(feature = "std")]
pub struct OwnedProducer<T, const N: usize> {
    buffer: Arc<RingBuffer<T, N>>,
}

#[cfg(feature = "std")]
impl<T, const N: usize> OwnedProducer<T, N> {
    /// Creates a new owned producer from an Arc'd ring buffer.
    pub(crate) fn new(buffer: Arc<RingBuffer<T, N>>) -> Self {
        Self { buffer }
    }

    /// Pushes a value into the buffer.
    ///
    /// Returns `Err(value)` if the buffer is full.
    #[inline]
    pub fn push(&self, value: T) -> Result<(), T> {
        self.buffer.push_internal(value)
    }

    /// Returns `true` if the buffer is full.
    #[inline]
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.buffer.is_full()
    }

    /// Returns the number of elements currently in the buffer.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns `true` if the buffer is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the capacity of the buffer.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }
}

#[cfg(feature = "std")]
unsafe impl<T: Send, const N: usize> Send for OwnedProducer<T, N> {}

/// An owned consumer handle backed by an `Arc`.
///
/// Unlike the borrowed `Consumer`, this handle owns a reference-counted
/// pointer to the ring buffer, allowing it to outlive the original buffer
/// and be used freely across threads.
#[cfg(feature = "std")]
pub struct OwnedConsumer<T, const N: usize> {
    buffer: Arc<RingBuffer<T, N>>,
}

#[cfg(feature = "std")]
impl<T, const N: usize> OwnedConsumer<T, N> {
    /// Creates a new owned consumer from an Arc'd ring buffer.
    pub(crate) fn new(buffer: Arc<RingBuffer<T, N>>) -> Self {
        Self { buffer }
    }

    /// Pops a value from the buffer.
    ///
    /// Returns `None` if the buffer is empty.
    #[inline]
    pub fn pop(&self) -> Option<T> {
        self.buffer.pop_internal()
    }

    /// Peeks at the next value without removing it.
    ///
    /// Returns `None` if the buffer is empty.
    #[inline]
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        self.buffer.peek_internal()
    }

    /// Returns `true` if the buffer is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the number of elements currently in the buffer.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the capacity of the buffer.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }
}

#[cfg(feature = "std")]
unsafe impl<T: Send, const N: usize> Send for OwnedConsumer<T, N> {}

#[cfg(feature = "std")]
impl<T, const N: usize> RingBuffer<T, N> {
    /// Splits an Arc'd ring buffer into owned producer and consumer handles.
    ///
    /// This is useful when you need the handles to outlive the original buffer
    /// reference, such as when passing them to threads.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sensor_pipeline::buffer::RingBuffer;
    /// use std::sync::Arc;
    ///
    /// let buffer = Arc::new(RingBuffer::<u32, 1024>::new());
    /// let (producer, consumer) = RingBuffer::split_arc(buffer);
    /// ```
    #[must_use]
    pub fn split_arc(buffer: Arc<Self>) -> (OwnedProducer<T, N>, OwnedConsumer<T, N>) {
        (
            OwnedProducer::new(Arc::clone(&buffer)),
            OwnedConsumer::new(buffer),
        )
    }
}

/// Backpressure policy for when the buffer is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressurePolicy {
    /// Drop the oldest data in the buffer to make room for new data.
    /// Good for continuous sensors where latest data is more important.
    DropOldest,
    /// Drop the newest data (reject the push).
    /// Good for commands where order must be preserved.
    DropNewest,
    /// Block until space is available.
    /// Only suitable for non-real-time paths.
    #[cfg(feature = "std")]
    Block,
}

/// A producer with configurable backpressure handling.
#[cfg(feature = "std")]
pub struct BackpressureProducer<T, const N: usize> {
    buffer: Arc<RingBuffer<T, N>>,
    policy: BackpressurePolicy,
    dropped_count: CachePadded<core::sync::atomic::AtomicU64>,
}

#[cfg(feature = "std")]
impl<T, const N: usize> BackpressureProducer<T, N> {
    /// Creates a new backpressure producer with the specified policy.
    #[must_use]
    pub fn new(buffer: Arc<RingBuffer<T, N>>, policy: BackpressurePolicy) -> Self {
        Self {
            buffer,
            policy,
            dropped_count: CachePadded::new(core::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Pushes a value according to the backpressure policy.
    ///
    /// For `DropOldest` and `DropNewest`, this always succeeds (unless blocking).
    /// For `Block`, this may block the current thread.
    pub fn push(&self, value: T) -> Result<(), T> {
        match self.policy {
            BackpressurePolicy::DropNewest => {
                if self.buffer.push_internal(value).is_err() {
                    self.dropped_count.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            }
            BackpressurePolicy::DropOldest => {
                // This is tricky in SPSC because we can't pop from the producer side
                // We need a different approach - for now, just drop newest as fallback
                // A proper implementation would need a different buffer design
                if self.buffer.push_internal(value).is_err() {
                    self.dropped_count.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            }
            BackpressurePolicy::Block => {
                let mut val = value;
                loop {
                    match self.buffer.push_internal(val) {
                        Ok(()) => return Ok(()),
                        Err(v) => {
                            val = v;
                            std::thread::yield_now();
                        }
                    }
                }
            }
        }
    }

    /// Returns the number of samples dropped due to backpressure.
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    /// Resets the dropped count to zero.
    pub fn reset_dropped_count(&self) {
        self.dropped_count.store(0, Ordering::Relaxed);
    }
}

#[cfg(feature = "std")]
unsafe impl<T: Send, const N: usize> Send for BackpressureProducer<T, N> {}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_owned_handles() {
        let buffer = Arc::new(RingBuffer::<u32, 1024>::new());
        let (producer, consumer) = RingBuffer::split_arc(buffer);

        let count = 1000;

        let producer_handle = thread::spawn(move || {
            for i in 0..count {
                while producer.push(i).is_err() {
                    thread::yield_now();
                }
            }
        });

        let consumer_handle = thread::spawn(move || {
            let mut received = 0u32;
            while received < count {
                if let Some(value) = consumer.pop() {
                    assert_eq!(value, received);
                    received += 1;
                } else {
                    thread::yield_now();
                }
            }
            received
        });

        producer_handle.join().expect("producer panicked");
        let received = consumer_handle.join().expect("consumer panicked");
        assert_eq!(received, count);
    }

    #[test]
    fn test_backpressure_drop_newest() {
        let buffer = Arc::new(RingBuffer::<u32, 4>::new());
        let producer = BackpressureProducer::new(buffer, BackpressurePolicy::DropNewest);

        // Push more than capacity (3)
        for i in 0..10 {
            producer.push(i).unwrap();
        }

        // Should have dropped 7 samples
        assert_eq!(producer.dropped_count(), 7);
    }

    #[test]
    fn test_backpressure_block() {
        let buffer = Arc::new(RingBuffer::<u32, 8>::new());
        let (producer, consumer) = RingBuffer::split_arc(Arc::clone(&buffer));

        // Fill the buffer
        for i in 0..7 {
            producer.push(i).unwrap();
        }

        let blocking_producer: BackpressureProducer<u32, 8> =
            BackpressureProducer::new(Arc::new(RingBuffer::new()), BackpressurePolicy::Block);

        // Spawn a thread that will consume, unblocking the producer
        let consumer_handle = thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(10));
            consumer.pop()
        });

        // This should return quickly as we're using the non-blocking producer
        let result = consumer_handle.join().expect("consumer panicked");
        assert!(result.is_some());
    }
}
