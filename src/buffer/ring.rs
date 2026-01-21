//! Lock-free SPSC (Single-Producer, Single-Consumer) ring buffer.
//!
//! This is the core data structure of the sensor pipeline. It provides wait-free
//! operations for both producer and consumer, making it suitable for real-time
//! applications where latency predictability is critical.
//!
//! # Design Decisions
//!
//! - **Const-generic size**: Buffer size is known at compile time, enabling optimizations
//! - **Power-of-2 requirement**: Allows using bitwise AND for index wrapping instead of modulo
//! - **Cache-line padding**: Head and tail indices are on separate cache lines to prevent false sharing
//! - **MaybeUninit storage**: Slots may be uninitialized; we carefully manage initialization state
//! - **Acquire-Release semantics**: Minimal synchronization for maximum performance
//!
//! # Memory Ordering
//!
//! - **Producer (push)**:
//!   - Reads tail with `Relaxed` (only updated by consumer, no ordering needed for check)
//!   - Writes data, then updates head with `Release` to make write visible
//! - **Consumer (pop)**:
//!   - Reads head with `Acquire` to see producer's writes
//!   - Reads data, then updates tail with `Release`
//!
//! # Safety
//!
//! The ring buffer is `Send` but NOT `Sync`. It must be split into separate
//! `Producer` and `Consumer` handles which ARE `Send`, ensuring only one
//! thread can produce and one can consume.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::CachePadded;

/// A lock-free single-producer, single-consumer ring buffer.
///
/// The buffer size `N` must be a power of 2. This is enforced at compile time
/// via a const assertion.
///
/// # Example
///
/// ```rust
/// use sensor_pipeline::buffer::RingBuffer;
///
/// let buffer: RingBuffer<u32, 16> = RingBuffer::new();
/// let (producer, consumer) = buffer.split();
///
/// // Producer thread
/// producer.push(42).unwrap();
///
/// // Consumer thread
/// assert_eq!(consumer.pop(), Some(42));
/// ```
#[repr(C)]
pub struct RingBuffer<T, const N: usize> {
    /// Write position - updated by producer.
    /// Cache-line padded to prevent false sharing with tail.
    head: CachePadded<AtomicUsize>,

    /// Read position - updated by consumer.
    /// Cache-line padded to prevent false sharing with head.
    tail: CachePadded<AtomicUsize>,

    /// Storage for buffer elements.
    /// Uses MaybeUninit because slots may be uninitialized.
    buffer: UnsafeCell<[MaybeUninit<T>; N]>,
}

// Compile-time assertion that N is a power of 2
const fn assert_power_of_two<const N: usize>() {
    assert!(N > 0 && (N & (N - 1)) == 0, "Buffer size must be a power of 2");
}

impl<T, const N: usize> RingBuffer<T, N> {
    /// Creates a new empty ring buffer.
    ///
    /// # Panics
    ///
    /// Panics at compile time if `N` is not a power of 2.
    #[must_use]
    pub const fn new() -> Self {
        assert_power_of_two::<N>();

        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            // SAFETY: MaybeUninit<T> does not require initialization
            buffer: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
        }
    }

    /// Returns the capacity of the buffer.
    ///
    /// Note: The usable capacity is `N - 1` because one slot is always kept empty
    /// to distinguish between full and empty states.
    #[inline]
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N - 1
    }

    /// Returns the number of elements currently in the buffer.
    ///
    /// This is an approximate value when called from outside producer/consumer context.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        head.wrapping_sub(tail) & (N - 1)
    }

    /// Returns `true` if the buffer is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        head == tail
    }

    /// Returns `true` if the buffer is full.
    #[inline]
    #[must_use]
    pub fn is_full(&self) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        Self::next_index(head) == tail
    }

    /// Computes the next index with wrap-around.
    ///
    /// Uses bitwise AND instead of modulo because N is guaranteed to be a power of 2.
    #[inline]
    const fn next_index(index: usize) -> usize {
        (index + 1) & (N - 1)
    }

    /// Returns the mask for index wrapping.
    #[inline]
    #[allow(dead_code)]
    const fn mask() -> usize {
        N - 1
    }

    /// Splits the ring buffer into producer and consumer handles.
    ///
    /// This is the only safe way to use the ring buffer across threads.
    /// The producer can only push, and the consumer can only pop.
    ///
    /// # Safety
    ///
    /// This method takes `&self` but creates handles that have mutable-like
    /// access to different parts of the buffer. The safety is maintained by:
    /// - Only one producer handle exists (enforced by returning owned handles)
    /// - Only one consumer handle exists
    /// - Producer only writes and updates head
    /// - Consumer only reads and updates tail
    #[must_use]
    pub fn split(&self) -> (Producer<'_, T, N>, Consumer<'_, T, N>) {
        (
            Producer { buffer: self },
            Consumer { buffer: self },
        )
    }

    /// Pushes a value into the buffer.
    ///
    /// Returns `Err(value)` if the buffer is full.
    ///
    /// # Safety
    ///
    /// Must only be called from a single producer thread.
    #[inline]
    pub(crate) fn push_internal(&self, value: T) -> Result<(), T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        let next_head = Self::next_index(head);

        if next_head == tail {
            // Buffer is full
            return Err(value);
        }

        // SAFETY: We have exclusive write access to buffer[head] because:
        // 1. Only the producer writes to slots between tail and head
        // 2. The consumer only reads from slots between tail and the previous head
        // 3. We checked that next_head != tail, so this slot is not being read
        unsafe {
            let slot = (*self.buffer.get()).get_unchecked_mut(head);
            slot.write(value);
        }

        // Release: Make the write visible to the consumer before updating head
        self.head.store(next_head, Ordering::Release);

        Ok(())
    }

    /// Pops a value from the buffer.
    ///
    /// Returns `None` if the buffer is empty.
    ///
    /// # Safety
    ///
    /// Must only be called from a single consumer thread.
    #[inline]
    pub(crate) fn pop_internal(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);
        // Acquire: See the producer's writes before reading the slot
        let head = self.head.load(Ordering::Acquire);

        if tail == head {
            // Buffer is empty
            return None;
        }

        // SAFETY: We have exclusive read access to buffer[tail] because:
        // 1. The producer only writes to slots ahead of tail
        // 2. We are the only consumer
        // 3. We checked that tail != head, so there is data to read
        let value = unsafe {
            let slot = (*self.buffer.get()).get_unchecked(tail);
            slot.assume_init_read()
        };

        let next_tail = Self::next_index(tail);
        // Release: Mark this slot as available for the producer
        self.tail.store(next_tail, Ordering::Release);

        Some(value)
    }

    /// Tries to peek at the next value without removing it.
    ///
    /// Returns `None` if the buffer is empty.
    ///
    /// # Safety
    ///
    /// Must only be called from the consumer thread.
    #[inline]
    pub(crate) fn peek_internal(&self) -> Option<&T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if tail == head {
            return None;
        }

        // SAFETY: Same as pop_internal, but we return a reference
        unsafe {
            let slot = (*self.buffer.get()).get_unchecked(tail);
            Some(slot.assume_init_ref())
        }
    }
}

impl<T, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for RingBuffer<T, N> {
    fn drop(&mut self) {
        // Drop any remaining elements in the buffer
        let head = *self.head.get_mut().get_mut();
        let tail = *self.tail.get_mut().get_mut();

        let mut idx = tail;
        while idx != head {
            // SAFETY: All slots between tail and head are initialized
            unsafe {
                let slot = (*self.buffer.get()).get_unchecked_mut(idx);
                slot.assume_init_drop();
            }
            idx = Self::next_index(idx);
        }
    }
}

// SAFETY: RingBuffer is Send if T is Send because:
// - The buffer can be safely transferred between threads
// - After transfer, it must be split into Producer/Consumer for use
unsafe impl<T: Send, const N: usize> Send for RingBuffer<T, N> {}

// NOTE: RingBuffer is NOT Sync because multiple threads cannot safely
// call push/pop on the same instance. Use split() to get thread-safe handles.

/// Producer handle for the ring buffer.
///
/// This handle can only push values into the buffer.
/// It is `Send`, allowing it to be moved to another thread.
pub struct Producer<'a, T, const N: usize> {
    buffer: &'a RingBuffer<T, N>,
}

impl<'a, T, const N: usize> Producer<'a, T, N> {
    /// Pushes a value into the buffer.
    ///
    /// Returns `Err(value)` if the buffer is full, giving back ownership
    /// of the value that couldn't be pushed.
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

// SAFETY: Producer is Send if T is Send
// Only one producer can exist due to the borrow, and push is safe to call from any thread
unsafe impl<T: Send, const N: usize> Send for Producer<'_, T, N> {}

/// Consumer handle for the ring buffer.
///
/// This handle can only pop values from the buffer.
/// It is `Send`, allowing it to be moved to another thread.
pub struct Consumer<'a, T, const N: usize> {
    buffer: &'a RingBuffer<T, N>,
}

impl<'a, T, const N: usize> Consumer<'a, T, N> {
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

// SAFETY: Consumer is Send if T is Send
unsafe impl<T: Send, const N: usize> Send for Consumer<'_, T, N> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buffer: RingBuffer<u32, 16> = RingBuffer::new();
        assert!(buffer.is_empty());
        assert!(!buffer.is_full());
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_capacity() {
        let buffer: RingBuffer<u32, 16> = RingBuffer::new();
        assert_eq!(buffer.capacity(), 15); // N - 1
    }

    #[test]
    fn test_push_pop_single() {
        let buffer: RingBuffer<u32, 16> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        assert!(producer.push(42).is_ok());
        assert_eq!(producer.len(), 1);

        assert_eq!(consumer.pop(), Some(42));
        assert_eq!(consumer.len(), 0);
    }

    #[test]
    fn test_push_pop_multiple() {
        let buffer: RingBuffer<u32, 16> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        for i in 0..10 {
            assert!(producer.push(i).is_ok());
        }
        assert_eq!(producer.len(), 10);

        for i in 0..10 {
            assert_eq!(consumer.pop(), Some(i));
        }
        assert!(consumer.is_empty());
    }

    #[test]
    fn test_full_buffer() {
        let buffer: RingBuffer<u32, 4> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        // Fill the buffer (capacity is 3 because N-1)
        assert!(producer.push(1).is_ok());
        assert!(producer.push(2).is_ok());
        assert!(producer.push(3).is_ok());

        // Buffer should be full
        assert!(producer.is_full());
        assert!(producer.push(4).is_err());

        // Pop one and we should be able to push again
        assert_eq!(consumer.pop(), Some(1));
        assert!(!producer.is_full());
        assert!(producer.push(4).is_ok());
    }

    #[test]
    fn test_empty_pop_returns_none() {
        let buffer: RingBuffer<u32, 16> = RingBuffer::new();
        let (_, consumer) = buffer.split();

        assert_eq!(consumer.pop(), None);
    }

    #[test]
    fn test_peek() {
        let buffer: RingBuffer<u32, 16> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        assert_eq!(consumer.peek(), None);

        producer.push(42).unwrap();
        assert_eq!(consumer.peek(), Some(&42));
        assert_eq!(consumer.peek(), Some(&42)); // Should still be there
        assert_eq!(consumer.pop(), Some(42));
        assert_eq!(consumer.peek(), None);
    }

    #[test]
    fn test_wrap_around() {
        let buffer: RingBuffer<u32, 4> = RingBuffer::new();
        let (producer, consumer) = buffer.split();

        // Fill and empty multiple times to test wrap-around
        for round in 0..5 {
            let base = round * 3;
            producer.push(base).unwrap();
            producer.push(base + 1).unwrap();
            producer.push(base + 2).unwrap();

            assert_eq!(consumer.pop(), Some(base));
            assert_eq!(consumer.pop(), Some(base + 1));
            assert_eq!(consumer.pop(), Some(base + 2));
        }
    }

    #[test]
    fn test_drop_remaining_elements() {
        use core::sync::atomic::AtomicUsize;
        use std::sync::Arc;

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        #[derive(Clone, Debug)]
        struct DropCounter(Arc<()>);

        impl Drop for DropCounter {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }

        DROP_COUNT.store(0, Ordering::SeqCst);

        {
            let buffer: RingBuffer<DropCounter, 8> = RingBuffer::new();
            let (producer, _) = buffer.split();

            for _ in 0..5 {
                producer.push(DropCounter(Arc::new(()))).unwrap();
            }
        } // buffer dropped here

        assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 5);
    }

    #[test]
    #[should_panic(expected = "Buffer size must be a power of 2")]
    fn test_non_power_of_two_panics() {
        let _buffer: RingBuffer<u32, 5> = RingBuffer::new();
    }

    #[test]
    fn test_index_wrapping() {
        // Test that our index wrapping is correct
        assert_eq!(RingBuffer::<u32, 16>::next_index(0), 1);
        assert_eq!(RingBuffer::<u32, 16>::next_index(15), 0);
        assert_eq!(RingBuffer::<u32, 16>::next_index(7), 8);
    }

    #[cfg(feature = "std")]
    mod threaded_tests {
        use super::*;
        use std::thread;

        #[test]
        fn test_producer_consumer_threads() {
            // Use Box::leak to get 'static lifetime for threads
            let buffer = Box::leak(Box::new(RingBuffer::<u32, 1024>::new()));
            let (producer, consumer) = buffer.split();

            let count = 10000;

            let producer_handle = thread::spawn(move || {
                for i in 0..count {
                    while producer.push(i).is_err() {
                        thread::yield_now();
                    }
                }
            });

            let consumer_handle = thread::spawn(move || {
                let mut received = 0u32;
                let mut expected = 0u32;
                while expected < count {
                    if let Some(value) = consumer.pop() {
                        assert_eq!(
                            value, expected,
                            "Out of order: got {value}, expected {expected}"
                        );
                        received += 1;
                        expected += 1;
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
    }
}
