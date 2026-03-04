//! Arc-wrapped sensor data for zero-copy sharing.
//!
//! This module provides `SharedData<T>` which wraps data in an Arc,
//! allowing it to be shared across pipeline stages without copying.

use std::ops::Deref;
use std::sync::Arc;

/// A wrapper for sharing data across pipeline stages without copying.
///
/// `SharedData<T>` uses reference counting to allow multiple stages
/// to access the same data without copying. This is particularly
/// useful for large sensor payloads like LIDAR point clouds or images.
///
/// # Example
///
/// ```rust
/// use sensor_bridge::zero_copy::SharedData;
///
/// // Create shared data
/// let data = SharedData::new(vec![1, 2, 3, 4, 5]);
///
/// // Clone is cheap (just increments reference count)
/// let data2 = data.clone();
///
/// // Both references point to the same data
/// assert_eq!(&*data, &*data2);
/// ```
#[derive(Debug)]
pub struct SharedData<T> {
    inner: Arc<T>,
}

impl<T> SharedData<T> {
    /// Creates new shared data.
    #[inline]
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(data),
        }
    }

    /// Creates shared data from an existing Arc.
    #[inline]
    pub fn from_arc(arc: Arc<T>) -> Self {
        Self { inner: arc }
    }

    /// Returns the inner Arc.
    #[inline]
    pub fn into_arc(self) -> Arc<T> {
        self.inner
    }

    /// Returns a reference to the inner Arc.
    #[inline]
    pub fn as_arc(&self) -> &Arc<T> {
        &self.inner
    }

    /// Returns the number of references to this data.
    #[inline]
    pub fn ref_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    /// Returns true if this is the only reference to the data.
    #[inline]
    pub fn is_unique(&self) -> bool {
        Arc::strong_count(&self.inner) == 1
    }

    /// Attempts to get a mutable reference to the data.
    ///
    /// Returns `Some(&mut T)` if this is the only reference,
    /// or `None` if there are other references.
    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        Arc::get_mut(&mut self.inner)
    }

    /// Makes the data mutable by cloning if necessary (copy-on-write).
    ///
    /// If this is the only reference, returns a mutable reference.
    /// If there are other references, clones the data and returns
    /// a mutable reference to the new copy.
    #[inline]
    pub fn make_mut(&mut self) -> &mut T
    where
        T: Clone,
    {
        Arc::make_mut(&mut self.inner)
    }

    /// Tries to unwrap the Arc, returning the inner data.
    ///
    /// Returns `Ok(T)` if this is the only reference,
    /// or `Err(Self)` if there are other references.
    #[inline]
    pub fn try_unwrap(self) -> Result<T, Self> {
        Arc::try_unwrap(self.inner).map_err(|inner| Self { inner })
    }

    /// Maps the shared data to a new type.
    ///
    /// If this is the only reference, the data is moved without cloning.
    /// Otherwise, the data is cloned first.
    #[inline]
    pub fn map<U, F>(self, f: F) -> SharedData<U>
    where
        T: Clone,
        F: FnOnce(T) -> U,
    {
        match self.try_unwrap() {
            Ok(data) => SharedData::new(f(data)),
            Err(shared) => SharedData::new(f((*shared.inner).clone())),
        }
    }
}

impl<T> Clone for SharedData<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Deref for SharedData<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> AsRef<T> for SharedData<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T: Default> Default for SharedData<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> From<T> for SharedData<T> {
    fn from(data: T) -> Self {
        Self::new(data)
    }
}

impl<T> From<Arc<T>> for SharedData<T> {
    fn from(arc: Arc<T>) -> Self {
        Self::from_arc(arc)
    }
}

impl<T: PartialEq> PartialEq for SharedData<T> {
    fn eq(&self, other: &Self) -> bool {
        // Check pointer equality first (fast path)
        Arc::ptr_eq(&self.inner, &other.inner) || *self.inner == *other.inner
    }
}

impl<T: Eq> Eq for SharedData<T> {}

impl<T: std::hash::Hash> std::hash::Hash for SharedData<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.inner.hash(state);
    }
}

// SAFETY: SharedData<T> is Send if T is Send + Sync (same as Arc<T>)
unsafe impl<T: Send + Sync> Send for SharedData<T> {}
unsafe impl<T: Send + Sync> Sync for SharedData<T> {}

/// A timestamped shared data wrapper.
///
/// Combines shared data with a timestamp for sensor readings.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TimestampedShared<T> {
    /// The shared data.
    pub data: SharedData<T>,
    /// Timestamp in nanoseconds.
    pub timestamp_ns: u64,
}

#[allow(dead_code)]
impl<T> TimestampedShared<T> {
    /// Creates new timestamped shared data.
    #[inline]
    pub fn new(data: T, timestamp_ns: u64) -> Self {
        Self {
            data: SharedData::new(data),
            timestamp_ns,
        }
    }

    /// Creates timestamped shared data from existing SharedData.
    #[inline]
    pub fn from_shared(data: SharedData<T>, timestamp_ns: u64) -> Self {
        Self { data, timestamp_ns }
    }
}

impl<T> Deref for TimestampedShared<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_data_basic() {
        let data = SharedData::new(42);
        assert_eq!(*data, 42);
        assert_eq!(data.ref_count(), 1);
        assert!(data.is_unique());
    }

    #[test]
    fn test_shared_data_clone() {
        let data1 = SharedData::new(vec![1, 2, 3]);
        let data2 = data1.clone();

        assert_eq!(*data1, *data2);
        assert_eq!(data1.ref_count(), 2);
        assert!(!data1.is_unique());

        drop(data2);
        assert_eq!(data1.ref_count(), 1);
        assert!(data1.is_unique());
    }

    #[test]
    fn test_shared_data_get_mut() {
        let mut data = SharedData::new(42);

        // Unique reference, can get mutable
        assert!(data.get_mut().is_some());
        *data.get_mut().unwrap() = 100;
        assert_eq!(*data, 100);

        // Clone creates another reference
        let _data2 = data.clone();
        assert!(data.get_mut().is_none());
    }

    #[test]
    fn test_shared_data_make_mut() {
        let mut data1 = SharedData::new(42);
        let data2 = data1.clone();

        // make_mut should clone since there are 2 references
        *data1.make_mut() = 100;

        // data1 now has its own copy
        assert_eq!(*data1, 100);
        assert_eq!(*data2, 42);
    }

    #[test]
    fn test_shared_data_try_unwrap() {
        let data = SharedData::new(42);

        // Should succeed with unique reference
        let value = data.try_unwrap().unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_shared_data_try_unwrap_fails() {
        let data1 = SharedData::new(42);
        let _data2 = data1.clone();

        // Should fail with multiple references
        let result = data1.try_unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn test_shared_data_map() {
        let data = SharedData::new(42);
        let mapped = data.map(|x| x * 2);
        assert_eq!(*mapped, 84);
    }

    #[test]
    fn test_shared_data_from() {
        let data: SharedData<i32> = 42.into();
        assert_eq!(*data, 42);

        let arc = Arc::new(100);
        let data2: SharedData<i32> = arc.into();
        assert_eq!(*data2, 100);
    }

    #[test]
    fn test_shared_data_eq() {
        let data1 = SharedData::new(42);
        let data2 = data1.clone();
        let data3 = SharedData::new(42);

        // Same Arc
        assert_eq!(data1, data2);
        // Different Arc, same value
        assert_eq!(data1, data3);
        // Different value
        assert_ne!(data1, SharedData::new(100));
    }

    #[test]
    fn test_timestamped_shared() {
        let ts = TimestampedShared::new(vec![1, 2, 3], 12345);
        assert_eq!(*ts, vec![1, 2, 3]);
        assert_eq!(ts.timestamp_ns, 12345);

        let _ts2 = ts.clone();
        assert_eq!(ts.data.ref_count(), 2);
    }

    #[test]
    fn test_shared_data_thread_safe() {
        use std::thread;

        let data = SharedData::new(42);
        let data2 = data.clone();

        let handle = thread::spawn(move || {
            assert_eq!(*data2, 42);
        });

        assert_eq!(*data, 42);
        handle.join().unwrap();
    }
}
