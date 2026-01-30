//! Pool for variable-size byte buffers.
//!
//! This module provides a buffer pool optimized for sensor data,
//! with support for different buffer sizes.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam::queue::ArrayQueue;

/// Size classes for buffer pooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferSize {
    /// Small buffers (256 bytes).
    Small,
    /// Medium buffers (4 KB).
    Medium,
    /// Large buffers (64 KB).
    Large,
    /// Extra large buffers (1 MB).
    ExtraLarge,
}

impl BufferSize {
    /// Returns the size in bytes.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        match self {
            Self::Small => 256,
            Self::Medium => 4 * 1024,
            Self::Large => 64 * 1024,
            Self::ExtraLarge => 1024 * 1024,
        }
    }

    /// Determines the appropriate size class for a given size.
    #[must_use]
    pub fn for_size(size: usize) -> Self {
        if size <= Self::Small.bytes() {
            Self::Small
        } else if size <= Self::Medium.bytes() {
            Self::Medium
        } else if size <= Self::Large.bytes() {
            Self::Large
        } else {
            Self::ExtraLarge
        }
    }
}

/// A pool for reusing byte buffers.
///
/// Maintains separate pools for different buffer sizes to minimize
/// waste and improve reuse rates.
pub struct BufferPool {
    small: ArrayQueue<Vec<u8>>,
    medium: ArrayQueue<Vec<u8>>,
    large: ArrayQueue<Vec<u8>>,
    extra_large: ArrayQueue<Vec<u8>>,
    pool_size: usize,
    allocated: AtomicUsize,
    reused: AtomicUsize,
}

impl BufferPool {
    /// Creates a new buffer pool.
    ///
    /// # Arguments
    ///
    /// * `pool_size` - Maximum number of buffers to keep per size class
    #[must_use]
    pub fn new(pool_size: usize) -> Arc<Self> {
        Arc::new(Self {
            small: ArrayQueue::new(pool_size),
            medium: ArrayQueue::new(pool_size),
            large: ArrayQueue::new(pool_size),
            extra_large: ArrayQueue::new(pool_size / 4), // Fewer large buffers
            pool_size,
            allocated: AtomicUsize::new(0),
            reused: AtomicUsize::new(0),
        })
    }

    /// Acquires a buffer of at least the specified size.
    pub fn acquire(self: &Arc<Self>, min_size: usize) -> PooledBuffer {
        let size_class = BufferSize::for_size(min_size);
        let queue = self.queue_for_size(size_class);

        let buffer = match queue.pop() {
            Some(mut buf) => {
                self.reused.fetch_add(1, Ordering::Relaxed);
                buf.clear();
                buf
            }
            None => {
                self.allocated.fetch_add(1, Ordering::Relaxed);
                Vec::with_capacity(size_class.bytes())
            }
        };

        PooledBuffer {
            pool: Arc::clone(self),
            buffer: Some(buffer),
            size_class,
        }
    }

    /// Acquires a buffer and fills it with the given data.
    pub fn acquire_with_data(self: &Arc<Self>, data: &[u8]) -> PooledBuffer {
        let mut buffer = self.acquire(data.len());
        buffer.extend_from_slice(data);
        buffer
    }

    /// Acquires a buffer of a specific size class.
    pub fn acquire_sized(self: &Arc<Self>, size_class: BufferSize) -> PooledBuffer {
        let queue = self.queue_for_size(size_class);

        let buffer = match queue.pop() {
            Some(mut buf) => {
                self.reused.fetch_add(1, Ordering::Relaxed);
                buf.clear();
                buf
            }
            None => {
                self.allocated.fetch_add(1, Ordering::Relaxed);
                Vec::with_capacity(size_class.bytes())
            }
        };

        PooledBuffer {
            pool: Arc::clone(self),
            buffer: Some(buffer),
            size_class,
        }
    }

    /// Pre-allocates buffers in each pool.
    pub fn prefill(self: &Arc<Self>, small: usize, medium: usize, large: usize) {
        for _ in 0..small.min(self.pool_size) {
            let _ = self.small.push(Vec::with_capacity(BufferSize::Small.bytes()));
        }
        for _ in 0..medium.min(self.pool_size) {
            let _ = self.medium.push(Vec::with_capacity(BufferSize::Medium.bytes()));
        }
        for _ in 0..large.min(self.pool_size) {
            let _ = self.large.push(Vec::with_capacity(BufferSize::Large.bytes()));
        }
    }

    fn queue_for_size(&self, size: BufferSize) -> &ArrayQueue<Vec<u8>> {
        match size {
            BufferSize::Small => &self.small,
            BufferSize::Medium => &self.medium,
            BufferSize::Large => &self.large,
            BufferSize::ExtraLarge => &self.extra_large,
        }
    }

    fn return_buffer(&self, mut buffer: Vec<u8>, size_class: BufferSize) {
        // Don't return oversized buffers to smaller pools
        if buffer.capacity() > size_class.bytes() * 2 {
            return;
        }

        buffer.clear();
        let _ = self.queue_for_size(size_class).push(buffer);
    }

    /// Returns the number of available small buffers.
    #[must_use]
    pub fn available_small(&self) -> usize {
        self.small.len()
    }

    /// Returns the number of available medium buffers.
    #[must_use]
    pub fn available_medium(&self) -> usize {
        self.medium.len()
    }

    /// Returns the number of available large buffers.
    #[must_use]
    pub fn available_large(&self) -> usize {
        self.large.len()
    }

    /// Returns the total number of buffers allocated.
    #[must_use]
    pub fn total_allocated(&self) -> usize {
        self.allocated.load(Ordering::Relaxed)
    }

    /// Returns the number of buffer reuses.
    #[must_use]
    pub fn total_reused(&self) -> usize {
        self.reused.load(Ordering::Relaxed)
    }

    /// Returns the reuse rate as a percentage.
    #[must_use]
    pub fn reuse_rate(&self) -> f64 {
        let allocated = self.total_allocated() as f64;
        let reused = self.total_reused() as f64;
        let total = allocated + reused;
        if total > 0.0 {
            reused / total * 100.0
        } else {
            0.0
        }
    }

    /// Returns pool statistics.
    #[must_use]
    pub fn stats(&self) -> BufferPoolStats {
        BufferPoolStats {
            small_available: self.small.len(),
            medium_available: self.medium.len(),
            large_available: self.large.len(),
            extra_large_available: self.extra_large.len(),
            total_allocated: self.total_allocated(),
            total_reused: self.total_reused(),
            reuse_rate: self.reuse_rate(),
        }
    }
}

/// A buffer acquired from the pool.
///
/// The buffer is automatically returned to the pool when dropped.
pub struct PooledBuffer {
    pool: Arc<BufferPool>,
    buffer: Option<Vec<u8>>,
    size_class: BufferSize,
}

impl PooledBuffer {
    /// Takes the buffer out of the guard without returning it to the pool.
    pub fn take(mut self) -> Vec<u8> {
        self.buffer.take().expect("buffer already taken")
    }

    /// Returns the size class of this buffer.
    #[must_use]
    pub fn size_class(&self) -> BufferSize {
        self.size_class
    }

    /// Returns the capacity of the buffer.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buffer.as_ref().map_or(0, |b| b.capacity())
    }

    /// Returns a reference to the pool.
    #[must_use]
    pub fn pool(&self) -> &Arc<BufferPool> {
        &self.pool
    }
}

impl Deref for PooledBuffer {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        self.buffer.as_ref().expect("buffer already taken")
    }
}

impl DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.buffer.as_mut().expect("buffer already taken")
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            self.pool.return_buffer(buffer, self.size_class);
        }
    }
}

impl std::io::Write for PooledBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.as_mut().expect("buffer already taken").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Statistics for the buffer pool.
#[derive(Debug, Clone)]
pub struct BufferPoolStats {
    /// Available small buffers.
    pub small_available: usize,
    /// Available medium buffers.
    pub medium_available: usize,
    /// Available large buffers.
    pub large_available: usize,
    /// Available extra large buffers.
    pub extra_large_available: usize,
    /// Total buffers allocated.
    pub total_allocated: usize,
    /// Total buffer reuses.
    pub total_reused: usize,
    /// Reuse rate as percentage.
    pub reuse_rate: f64,
}

impl std::fmt::Display for BufferPoolStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BufferPool: available(S:{} M:{} L:{} XL:{}) allocated:{} reused:{} rate:{:.1}%",
            self.small_available,
            self.medium_available,
            self.large_available,
            self.extra_large_available,
            self.total_allocated,
            self.total_reused,
            self.reuse_rate
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_buffer_size() {
        assert_eq!(BufferSize::Small.bytes(), 256);
        assert_eq!(BufferSize::Medium.bytes(), 4096);
        assert_eq!(BufferSize::Large.bytes(), 65536);

        assert_eq!(BufferSize::for_size(100), BufferSize::Small);
        assert_eq!(BufferSize::for_size(1000), BufferSize::Medium);
        assert_eq!(BufferSize::for_size(10000), BufferSize::Large);
        assert_eq!(BufferSize::for_size(100000), BufferSize::ExtraLarge);
    }

    #[test]
    fn test_buffer_pool_basic() {
        let pool = BufferPool::new(10);

        let mut buf = pool.acquire(100);
        buf.extend_from_slice(b"hello");
        assert_eq!(&*buf, b"hello");
        assert!(buf.capacity() >= BufferSize::Small.bytes());

        drop(buf);

        assert_eq!(pool.available_small(), 1);
    }

    #[test]
    fn test_buffer_pool_reuse() {
        let pool = BufferPool::new(10);

        {
            let mut buf = pool.acquire(100);
            buf.extend_from_slice(b"test data");
        }

        assert_eq!(pool.total_allocated(), 1);

        {
            let buf = pool.acquire(100);
            // Buffer should be empty (cleared on return)
            assert!(buf.is_empty());
        }

        assert_eq!(pool.total_allocated(), 1);
        assert_eq!(pool.total_reused(), 1);
    }

    #[test]
    fn test_buffer_pool_size_classes() {
        let pool = BufferPool::new(10);

        let small = pool.acquire(100);
        let medium = pool.acquire(1000);
        let large = pool.acquire(10000);

        assert_eq!(small.size_class(), BufferSize::Small);
        assert_eq!(medium.size_class(), BufferSize::Medium);
        assert_eq!(large.size_class(), BufferSize::Large);
    }

    #[test]
    fn test_buffer_pool_prefill() {
        let pool = BufferPool::new(10);
        pool.prefill(5, 3, 1);

        assert_eq!(pool.available_small(), 5);
        assert_eq!(pool.available_medium(), 3);
        assert_eq!(pool.available_large(), 1);
    }

    #[test]
    fn test_buffer_pool_acquire_with_data() {
        let pool = BufferPool::new(10);
        let data = b"sensor reading data";

        let buf = pool.acquire_with_data(data);
        assert_eq!(&*buf, data);
    }

    #[test]
    fn test_buffer_pool_take() {
        let pool = BufferPool::new(10);
        pool.prefill(1, 0, 0);

        let buf = pool.acquire(100);
        let vec = buf.take();

        // Buffer was not returned to pool
        assert_eq!(pool.available_small(), 0);
        assert!(vec.is_empty());
    }

    #[test]
    fn test_buffer_pool_concurrent() {
        let pool = BufferPool::new(100);
        pool.prefill(50, 25, 10);

        let mut handles = vec![];

        for _ in 0..4 {
            let pool = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    let mut buf = pool.acquire(100);
                    buf.extend_from_slice(b"data");
                    thread::yield_now();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // All buffers should be back in pool
        assert!(pool.reuse_rate() > 0.0);
    }

    #[test]
    fn test_buffer_pool_stats() {
        let pool = BufferPool::new(10);
        pool.prefill(5, 3, 1);

        let stats = pool.stats();
        assert_eq!(stats.small_available, 5);
        assert_eq!(stats.medium_available, 3);
        assert_eq!(stats.large_available, 1);
    }

    #[test]
    fn test_pooled_buffer_write() {
        use std::io::Write;

        let pool = BufferPool::new(10);
        let mut buf = pool.acquire(100);

        write!(buf, "Hello, {}!", "world").unwrap();
        assert_eq!(&*buf, b"Hello, world!");
    }
}
