//! Generic object pool with RAII guards.
//!
//! This module provides a lock-free object pool for reusing objects
//! without repeated allocations.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam::queue::ArrayQueue;

/// A thread-safe object pool for reusing objects.
///
/// Objects are acquired from the pool and automatically returned
/// when the guard is dropped. If the pool is empty, new objects
/// are created using the provided factory function.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::zero_copy::ObjectPool;
///
/// #[derive(Default)]
/// struct SensorReading {
///     timestamp: u64,
///     value: f64,
/// }
///
/// let pool = ObjectPool::new(|| SensorReading::default(), 100);
///
/// // Acquire an object
/// let mut reading = pool.acquire();
/// reading.timestamp = 12345;
/// reading.value = 42.0;
///
/// // Object is returned to pool when dropped
/// drop(reading);
///
/// // Next acquire may return the same object
/// let reading2 = pool.acquire();
/// ```
pub struct ObjectPool<T> {
    /// The underlying queue of pooled objects.
    queue: ArrayQueue<T>,
    /// Factory function for creating new objects.
    factory: Box<dyn Fn() -> T + Send + Sync>,
    /// Reset function called when returning objects to pool.
    reset: Option<Box<dyn Fn(&mut T) + Send + Sync>>,
    /// Number of objects currently acquired.
    acquired_count: AtomicUsize,
    /// Total objects created (including initial pool).
    total_created: AtomicUsize,
    /// Pool capacity.
    capacity: usize,
}

impl<T> ObjectPool<T> {
    /// Creates a new object pool.
    ///
    /// # Arguments
    ///
    /// * `factory` - Function to create new objects
    /// * `capacity` - Maximum pool size
    pub fn new<F>(factory: F, capacity: usize) -> Arc<Self>
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        let queue = ArrayQueue::new(capacity);
        Arc::new(Self {
            queue,
            factory: Box::new(factory),
            reset: None,
            acquired_count: AtomicUsize::new(0),
            total_created: AtomicUsize::new(0),
            capacity,
        })
    }

    /// Creates a new object pool with a reset function.
    ///
    /// The reset function is called on objects before they are returned
    /// to the pool, allowing them to be reused in a clean state.
    pub fn with_reset<F, R>(factory: F, reset: R, capacity: usize) -> Arc<Self>
    where
        F: Fn() -> T + Send + Sync + 'static,
        R: Fn(&mut T) + Send + Sync + 'static,
    {
        let queue = ArrayQueue::new(capacity);
        Arc::new(Self {
            queue,
            factory: Box::new(factory),
            reset: Some(Box::new(reset)),
            acquired_count: AtomicUsize::new(0),
            total_created: AtomicUsize::new(0),
            capacity,
        })
    }

    /// Pre-populates the pool with objects.
    pub fn prefill(self: &Arc<Self>, count: usize) {
        for _ in 0..count.min(self.capacity) {
            let obj = (self.factory)();
            self.total_created.fetch_add(1, Ordering::Relaxed);
            let _ = self.queue.push(obj);
        }
    }

    /// Acquires an object from the pool.
    ///
    /// Returns a pooled object if available, or creates a new one.
    pub fn acquire(self: &Arc<Self>) -> PoolGuard<T> {
        self.acquired_count.fetch_add(1, Ordering::Relaxed);

        let obj = match self.queue.pop() {
            Some(obj) => obj,
            None => {
                self.total_created.fetch_add(1, Ordering::Relaxed);
                (self.factory)()
            }
        };

        PoolGuard {
            pool: Arc::clone(self),
            obj: Some(obj),
        }
    }

    /// Tries to acquire an object without creating a new one.
    ///
    /// Returns `Some(guard)` if an object was available, `None` otherwise.
    pub fn try_acquire(self: &Arc<Self>) -> Option<PoolGuard<T>> {
        self.queue.pop().map(|obj| {
            self.acquired_count.fetch_add(1, Ordering::Relaxed);
            PoolGuard {
                pool: Arc::clone(self),
                obj: Some(obj),
            }
        })
    }

    /// Returns an object to the pool.
    fn return_object(&self, mut obj: T) {
        self.acquired_count.fetch_sub(1, Ordering::Relaxed);

        // Reset the object if a reset function is provided
        if let Some(reset) = &self.reset {
            reset(&mut obj);
        }

        // Try to return to pool (may fail if pool is full)
        let _ = self.queue.push(obj);
    }

    /// Returns the number of objects currently in the pool.
    #[must_use]
    pub fn available(&self) -> usize {
        self.queue.len()
    }

    /// Returns the number of objects currently acquired.
    #[must_use]
    pub fn acquired(&self) -> usize {
        self.acquired_count.load(Ordering::Relaxed)
    }

    /// Returns the total number of objects created.
    #[must_use]
    pub fn total_created(&self) -> usize {
        self.total_created.load(Ordering::Relaxed)
    }

    /// Returns the pool capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the pool utilization as a percentage.
    #[must_use]
    pub fn utilization(&self) -> f64 {
        let available = self.available();
        if self.capacity == 0 {
            0.0
        } else {
            (self.capacity - available) as f64 / self.capacity as f64 * 100.0
        }
    }
}

/// A guard that holds a pooled object and returns it when dropped.
pub struct PoolGuard<T> {
    pool: Arc<ObjectPool<T>>,
    obj: Option<T>,
}

impl<T> PoolGuard<T> {
    /// Takes the object out of the guard without returning it to the pool.
    ///
    /// This is useful when you want to keep the object permanently.
    pub fn take(mut self) -> T {
        self.obj.take().expect("object already taken")
    }

    /// Returns a reference to the underlying pool.
    pub fn pool(&self) -> &Arc<ObjectPool<T>> {
        &self.pool
    }
}

impl<T> Deref for PoolGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.obj.as_ref().expect("object already taken")
    }
}

impl<T> DerefMut for PoolGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.obj.as_mut().expect("object already taken")
    }
}

impl<T> Drop for PoolGuard<T> {
    fn drop(&mut self) {
        if let Some(obj) = self.obj.take() {
            self.pool.return_object(obj);
        }
    }
}

/// A simple pooled object wrapper for non-Arc usage.
pub struct PooledObject<T> {
    obj: T,
    was_pooled: bool,
}

impl<T> PooledObject<T> {
    /// Creates a new pooled object wrapper.
    pub fn new(obj: T) -> Self {
        Self {
            obj,
            was_pooled: false,
        }
    }

    /// Creates a pooled object wrapper marking it as from a pool.
    pub fn from_pool(obj: T) -> Self {
        Self {
            obj,
            was_pooled: true,
        }
    }

    /// Returns whether this object came from a pool.
    pub fn was_pooled(&self) -> bool {
        self.was_pooled
    }

    /// Consumes the wrapper and returns the inner object.
    pub fn into_inner(self) -> T {
        self.obj
    }
}

impl<T> Deref for PooledObject<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.obj
    }
}

impl<T> DerefMut for PooledObject<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.obj
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::thread;

    #[derive(Default)]
    struct TestObject {
        value: i32,
    }

    #[test]
    fn test_object_pool_basic() {
        let pool = ObjectPool::new(TestObject::default, 10);
        pool.prefill(5);

        assert_eq!(pool.available(), 5);
        assert_eq!(pool.acquired(), 0);

        {
            let mut obj = pool.acquire();
            obj.value = 42;
            assert_eq!(pool.acquired(), 1);
            assert_eq!(pool.available(), 4);
        }

        // Object should be returned to pool
        assert_eq!(pool.acquired(), 0);
        assert_eq!(pool.available(), 5);
    }

    #[test]
    fn test_object_pool_reuse() {
        let pool = ObjectPool::new(TestObject::default, 10);

        // Acquire and set value
        {
            let mut obj = pool.acquire();
            obj.value = 42;
        }

        // Next acquire should get the same object (with same value)
        let obj = pool.acquire();
        assert_eq!(obj.value, 42);
    }

    #[test]
    fn test_object_pool_with_reset() {
        let pool = ObjectPool::with_reset(TestObject::default, |obj| obj.value = 0, 10);

        {
            let mut obj = pool.acquire();
            obj.value = 42;
        }

        // Next acquire should get reset object
        let obj = pool.acquire();
        assert_eq!(obj.value, 0);
    }

    #[test]
    fn test_object_pool_overflow() {
        let pool = ObjectPool::new(TestObject::default, 2);
        pool.prefill(2);

        // Acquire all objects
        let obj1 = pool.acquire();
        let obj2 = pool.acquire();
        let obj3 = pool.acquire(); // Creates new object

        assert_eq!(pool.available(), 0);
        assert_eq!(pool.acquired(), 3);
        assert_eq!(pool.total_created(), 3);

        drop(obj1);
        drop(obj2);
        drop(obj3); // Pool is full, this one is dropped

        assert_eq!(pool.available(), 2);
        assert_eq!(pool.acquired(), 0);
    }

    #[test]
    fn test_object_pool_try_acquire() {
        let pool = ObjectPool::new(TestObject::default, 10);

        // Empty pool
        assert!(pool.try_acquire().is_none());

        pool.prefill(1);
        // Hold the guard so it doesn't return to the pool
        let _guard = pool.try_acquire();
        assert!(_guard.is_some());
        // Now pool should be empty
        assert!(pool.try_acquire().is_none());
    }

    #[test]
    fn test_object_pool_take() {
        let pool = ObjectPool::new(TestObject::default, 10);
        pool.prefill(1);

        let guard = pool.acquire();
        let obj = guard.take();

        // Object was not returned to pool
        assert_eq!(pool.available(), 0);
        assert_eq!(obj.value, 0);
    }

    #[test]
    fn test_object_pool_concurrent() {
        let pool = ObjectPool::new(TestObject::default, 100);
        pool.prefill(50);

        let mut handles = vec![];

        for _ in 0..4 {
            let pool = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    let mut obj = pool.acquire();
                    obj.value += 1;
                    thread::yield_now();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(pool.acquired(), 0);
    }

    #[test]
    fn test_object_pool_utilization() {
        let pool = ObjectPool::new(TestObject::default, 100);
        pool.prefill(100);

        assert_eq!(pool.utilization(), 0.0);

        let _guards: Vec<_> = (0..50).map(|_| pool.acquire()).collect();

        assert!((pool.utilization() - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_pooled_object() {
        let obj = PooledObject::new(42);
        assert!(!obj.was_pooled());
        assert_eq!(*obj, 42);

        let obj2 = PooledObject::from_pool(100);
        assert!(obj2.was_pooled());
        assert_eq!(obj2.into_inner(), 100);
    }
}
