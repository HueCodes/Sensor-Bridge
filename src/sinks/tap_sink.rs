//! Non-blocking "tap" sink that keeps the latest N items.
//!
//! Useful for diagnostics, a live UI polling the most recent values, or
//! a CLI subcommand that wants to peek at the stream without stopping
//! the pipeline. Readers are non-blocking: a call to [`TapSink::latest`]
//! clones out a snapshot of the current ring buffer.

use std::sync::Mutex;

use crate::error::Result;

use super::Sink;

/// Bounded ring-buffer sink retaining the most recent `capacity` items.
///
/// Pushes evict the oldest item. Concurrent readers go through a single
/// mutex; this is not a real-time reader path, it exists for
/// introspection and human-paced polling.
pub struct TapSink<T: Clone + Send> {
    capacity: usize,
    inner: Mutex<Vec<T>>,
}

impl<T: Clone + Send> TapSink<T> {
    /// Creates a tap with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        Self {
            capacity,
            inner: Mutex::new(Vec::with_capacity(capacity)),
        }
    }

    /// Returns the number of items currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Returns `true` if the buffer currently has no items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a snapshot of the current buffer in arrival order (oldest
    /// first). Clones all retained items.
    #[must_use]
    pub fn latest(&self) -> Vec<T> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Returns the most recently written item, if any.
    #[must_use]
    pub fn last(&self) -> Option<T> {
        self.inner.lock().ok().and_then(|g| g.last().cloned())
    }
}

impl<T: Clone + Send> Sink<T> for TapSink<T> {
    fn write(&mut self, item: T) -> Result<()> {
        if let Ok(mut g) = self.inner.lock() {
            if g.len() == self.capacity {
                g.remove(0);
            }
            g.push(item);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_latest_n_items() {
        let mut tap = TapSink::<u32>::new(3);
        for i in 0..5 {
            tap.write(i).unwrap();
        }
        assert_eq!(tap.latest(), vec![2, 3, 4]);
        assert_eq!(tap.last(), Some(4));
    }

    #[test]
    fn empty_initially() {
        let tap = TapSink::<u32>::new(4);
        assert!(tap.is_empty());
        assert_eq!(tap.last(), None);
    }
}
