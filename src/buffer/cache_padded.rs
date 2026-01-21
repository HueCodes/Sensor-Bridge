//! Cache line padding to prevent false sharing.
//!
//! False sharing occurs when two threads access different variables that happen
//! to be on the same CPU cache line. When one thread writes, it invalidates the
//! entire cache line, forcing the other thread to reload it even though it's
//! accessing a different variable.
//!
//! This module provides [`CachePadded<T>`] which ensures the wrapped value is
//! aligned to cache line boundaries and padded to fill an entire cache line.
//!
//! # Cache Line Sizes
//!
//! - x86-64: 64 bytes
//! - ARM64 (most): 64 bytes
//! - Apple Silicon: 128 bytes (has 128-byte prefetch)
//!
//! We use 128 bytes to be safe on all modern platforms.

use core::fmt;
use core::ops::{Deref, DerefMut};

/// Cache line size for most modern architectures.
///
/// We use 128 bytes to accommodate Apple Silicon's larger cache lines
/// and prefetch behavior, while still working correctly on x86-64 and
/// standard ARM64.
pub const CACHE_LINE_SIZE: usize = 128;

/// A wrapper that aligns and pads its contents to cache line boundaries.
///
/// This prevents false sharing when multiple `CachePadded` values are stored
/// contiguously in memory and accessed by different threads.
///
/// # Example
///
/// ```rust
/// use sensor_pipeline::buffer::CachePadded;
/// use core::sync::atomic::{AtomicUsize, Ordering};
///
/// struct SharedCounters {
///     // These won't cause false sharing between producer and consumer
///     producer_count: CachePadded<AtomicUsize>,
///     consumer_count: CachePadded<AtomicUsize>,
/// }
///
/// let counters = SharedCounters {
///     producer_count: CachePadded::new(AtomicUsize::new(0)),
///     consumer_count: CachePadded::new(AtomicUsize::new(0)),
/// };
///
/// // Different threads can access these without false sharing
/// counters.producer_count.store(1, Ordering::Relaxed);
/// counters.consumer_count.store(2, Ordering::Relaxed);
/// ```
#[repr(C, align(128))]
pub struct CachePadded<T> {
    value: T,
    // Padding is implicit due to repr(align(128))
}

impl<T> CachePadded<T> {
    /// Creates a new cache-padded value.
    #[inline]
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self { value }
    }

    /// Consumes the `CachePadded` and returns the inner value.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Returns a reference to the inner value.
    #[inline]
    #[must_use]
    pub const fn get(&self) -> &T {
        &self.value
    }

    /// Returns a mutable reference to the inner value.
    #[inline]
    #[must_use]
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for CachePadded<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T: Default> Default for CachePadded<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone> Clone for CachePadded<T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<T: Copy> Copy for CachePadded<T> {}

impl<T: fmt::Debug> fmt::Debug for CachePadded<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CachePadded").field(&self.value).finish()
    }
}

impl<T: PartialEq> PartialEq for CachePadded<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Eq> Eq for CachePadded<T> {}

impl<T> From<T> for CachePadded<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

// SAFETY: CachePadded is Send/Sync if T is
unsafe impl<T: Send> Send for CachePadded<T> {}
unsafe impl<T: Sync> Sync for CachePadded<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;
    use core::sync::atomic::AtomicUsize;

    #[test]
    fn test_cache_line_alignment() {
        let padded = CachePadded::new(42u64);
        let addr = &padded as *const _ as usize;
        assert_eq!(
            addr % CACHE_LINE_SIZE,
            0,
            "CachePadded should be aligned to cache line boundary"
        );
    }

    #[test]
    fn test_cache_line_size() {
        // Verify that CachePadded is at least CACHE_LINE_SIZE bytes
        assert!(
            mem::size_of::<CachePadded<u64>>() >= CACHE_LINE_SIZE,
            "CachePadded should be at least {} bytes, but is {}",
            CACHE_LINE_SIZE,
            mem::size_of::<CachePadded<u64>>()
        );
    }

    #[test]
    fn test_no_false_sharing_layout() {
        // Two adjacent CachePadded values should not share a cache line
        struct Adjacent {
            a: CachePadded<AtomicUsize>,
            b: CachePadded<AtomicUsize>,
        }

        let adj = Adjacent {
            a: CachePadded::new(AtomicUsize::new(0)),
            b: CachePadded::new(AtomicUsize::new(0)),
        };

        let addr_a = &adj.a as *const _ as usize;
        let addr_b = &adj.b as *const _ as usize;

        // They should be at least CACHE_LINE_SIZE apart
        assert!(
            addr_b.abs_diff(addr_a) >= CACHE_LINE_SIZE,
            "Adjacent CachePadded values should be on different cache lines"
        );
    }

    #[test]
    fn test_deref() {
        let padded = CachePadded::new(42u64);
        assert_eq!(*padded, 42);
    }

    #[test]
    fn test_deref_mut() {
        let mut padded = CachePadded::new(42u64);
        *padded = 100;
        assert_eq!(*padded, 100);
    }

    #[test]
    fn test_into_inner() {
        let padded = CachePadded::new(String::from("hello"));
        let inner = padded.into_inner();
        assert_eq!(inner, "hello");
    }

    #[test]
    fn test_default() {
        let padded: CachePadded<u64> = CachePadded::default();
        assert_eq!(*padded, 0);
    }

    #[test]
    fn test_from() {
        let padded: CachePadded<u64> = 42u64.into();
        assert_eq!(*padded, 42);
    }

    #[test]
    fn test_clone() {
        let original = CachePadded::new(42u64);
        let cloned = original.clone();
        assert_eq!(*original, *cloned);
    }
}
