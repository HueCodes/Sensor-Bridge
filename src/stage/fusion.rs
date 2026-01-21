//! Multi-sensor fusion stages.
//!
//! This module provides stages for combining data from multiple sensors,
//! including timestamp-based alignment and data fusion.

use crate::timestamp::Timestamped;

use super::Stage;

/// A stage that buffers timestamped data for synchronization.
///
/// Holds data until it can be paired with data from another sensor
/// within a timestamp tolerance window.
#[derive(Debug)]
pub struct TimestampBuffer<T, const CAPACITY: usize> {
    buffer: [Option<Timestamped<T>>; CAPACITY],
    write_idx: usize,
    count: usize,
}

impl<T: Copy, const CAPACITY: usize> TimestampBuffer<T, CAPACITY> {
    /// Creates a new timestamp buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: [None; CAPACITY],
            write_idx: 0,
            count: 0,
        }
    }

    /// Adds a new timestamped value to the buffer.
    pub fn push(&mut self, value: Timestamped<T>) {
        self.buffer[self.write_idx] = Some(value);
        self.write_idx = (self.write_idx + 1) % CAPACITY;
        if self.count < CAPACITY {
            self.count += 1;
        }
    }

    /// Finds the value with timestamp closest to the target.
    ///
    /// Returns `None` if no value is within the tolerance.
    pub fn find_closest(&self, target_ns: u64, tolerance_ns: u64) -> Option<&Timestamped<T>> {
        let mut best: Option<&Timestamped<T>> = None;
        let mut best_diff = u64::MAX;

        for slot in &self.buffer {
            if let Some(ref item) = slot {
                let diff = target_ns.abs_diff(item.timestamp_ns);
                if diff <= tolerance_ns && diff < best_diff {
                    best_diff = diff;
                    best = Some(item);
                }
            }
        }

        best
    }

    /// Removes values older than the given timestamp.
    pub fn prune_before(&mut self, timestamp_ns: u64) {
        for slot in &mut self.buffer {
            if let Some(ref item) = slot {
                if item.timestamp_ns < timestamp_ns {
                    *slot = None;
                    if self.count > 0 {
                        self.count -= 1;
                    }
                }
            }
        }
    }

    /// Returns the number of items in the buffer.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if the buffer is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clears all items from the buffer.
    pub fn clear(&mut self) {
        for slot in &mut self.buffer {
            *slot = None;
        }
        self.count = 0;
        self.write_idx = 0;
    }
}

impl<T: Copy, const CAPACITY: usize> Default for TimestampBuffer<T, CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

/// A pair of synchronized sensor readings.
#[derive(Debug, Clone, Copy)]
pub struct SyncedPair<A, B> {
    /// The first sensor reading (used as reference timestamp).
    pub primary: Timestamped<A>,
    /// The second sensor reading (matched by timestamp).
    pub secondary: Timestamped<B>,
    /// The time difference in nanoseconds between the two readings.
    pub time_diff_ns: u64,
}

impl<A, B> SyncedPair<A, B> {
    /// Creates a new synchronized pair.
    #[must_use]
    pub fn new(primary: Timestamped<A>, secondary: Timestamped<B>) -> Self {
        let time_diff_ns = primary.timestamp_ns.abs_diff(secondary.timestamp_ns);
        Self {
            primary,
            secondary,
            time_diff_ns,
        }
    }

    /// Returns the reference timestamp (from the primary sensor).
    #[must_use]
    pub const fn timestamp_ns(&self) -> u64 {
        self.primary.timestamp_ns
    }
}

/// A stage that synchronizes two sensor streams by timestamp.
///
/// Takes input from a primary sensor and matches it with buffered data
/// from a secondary sensor within a tolerance window.
///
/// # Synchronization Strategy
///
/// 1. Primary sensor data triggers output
/// 2. Secondary data is buffered
/// 3. When primary data arrives, find closest secondary data within tolerance
/// 4. Output synchronized pair if match found
#[derive(Debug)]
pub struct TimestampSync<A, B, const CAPACITY: usize> {
    secondary_buffer: TimestampBuffer<B, CAPACITY>,
    tolerance_ns: u64,
    /// Number of primary samples that couldn't be matched.
    pub unmatched_count: u64,
    /// Marker for the primary type.
    _phantom: core::marker::PhantomData<A>,
}

impl<A: Copy + Send, B: Copy + Send, const CAPACITY: usize> TimestampSync<A, B, CAPACITY> {
    /// Creates a new timestamp synchronization stage.
    ///
    /// # Arguments
    ///
    /// * `tolerance_ns` - Maximum time difference for matching samples
    #[must_use]
    pub const fn new(tolerance_ns: u64) -> Self {
        Self {
            secondary_buffer: TimestampBuffer::new(),
            tolerance_ns,
            unmatched_count: 0,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Creates with a tolerance in microseconds.
    #[must_use]
    pub const fn with_tolerance_us(tolerance_us: u64) -> Self {
        Self::new(tolerance_us * 1000)
    }

    /// Creates with a tolerance in milliseconds.
    #[must_use]
    pub const fn with_tolerance_ms(tolerance_ms: u64) -> Self {
        Self::new(tolerance_ms * 1_000_000)
    }

    /// Adds secondary sensor data to the buffer.
    pub fn push_secondary(&mut self, data: Timestamped<B>) {
        self.secondary_buffer.push(data);
    }

    /// Attempts to match primary data with buffered secondary data.
    pub fn match_primary(&mut self, primary: Timestamped<A>) -> Option<SyncedPair<A, B>> {
        // Find closest secondary reading
        if let Some(secondary) = self
            .secondary_buffer
            .find_closest(primary.timestamp_ns, self.tolerance_ns)
        {
            let pair = SyncedPair::new(primary, *secondary);

            // Prune old data from buffer
            let cutoff = primary.timestamp_ns.saturating_sub(self.tolerance_ns);
            self.secondary_buffer.prune_before(cutoff);

            Some(pair)
        } else {
            self.unmatched_count += 1;
            None
        }
    }

    /// Returns the current buffer size.
    #[must_use]
    pub fn buffer_len(&self) -> usize {
        self.secondary_buffer.len()
    }
}

/// Input for the timestamp sync stage.
#[derive(Debug, Clone, Copy)]
pub enum SyncInput<A, B> {
    /// Primary sensor data (triggers output).
    Primary(Timestamped<A>),
    /// Secondary sensor data (buffered).
    Secondary(Timestamped<B>),
}

impl<A: Copy + Send, B: Copy + Send, const CAPACITY: usize> Stage
    for TimestampSync<A, B, CAPACITY>
{
    type Input = SyncInput<A, B>;
    type Output = SyncedPair<A, B>;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        match input {
            SyncInput::Primary(data) => self.match_primary(data),
            SyncInput::Secondary(data) => {
                self.push_secondary(data);
                None
            }
        }
    }

    fn reset(&mut self) {
        self.secondary_buffer.clear();
        self.unmatched_count = 0;
    }
}

/// A simple data fusion stage that combines two values using a function.
#[derive(Debug)]
pub struct Fuse<A, B, O, F>
where
    F: FnMut(A, B) -> O,
{
    fuse_fn: F,
    _phantom: core::marker::PhantomData<(A, B, O)>,
}

impl<A, B, O, F> Fuse<A, B, O, F>
where
    F: FnMut(A, B) -> O,
{
    /// Creates a new fusion stage with the given function.
    pub const fn new(fuse_fn: F) -> Self {
        Self {
            fuse_fn,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<A: Send, B: Send, O: Send, F: FnMut(A, B) -> O + Send> Stage for Fuse<A, B, O, F> {
    type Input = (A, B);
    type Output = O;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some((self.fuse_fn)(input.0, input.1))
    }

    fn reset(&mut self) {}
}

/// A weighted average fusion strategy.
#[derive(Debug, Clone)]
pub struct WeightedAverage {
    weight_a: f32,
    weight_b: f32,
}

impl WeightedAverage {
    /// Creates a new weighted average with the given weights.
    ///
    /// Weights are normalized to sum to 1.0.
    #[must_use]
    pub fn new(weight_a: f32, weight_b: f32) -> Self {
        let sum = weight_a + weight_b;
        Self {
            weight_a: weight_a / sum,
            weight_b: weight_b / sum,
        }
    }

    /// Creates equal weights (simple average).
    #[must_use]
    pub fn equal() -> Self {
        Self::new(1.0, 1.0)
    }

    /// Fuses two f32 values.
    #[must_use]
    pub fn fuse_f32(&self, a: f32, b: f32) -> f32 {
        a * self.weight_a + b * self.weight_b
    }

    /// Fuses two f64 values.
    #[must_use]
    pub fn fuse_f64(&self, a: f64, b: f64) -> f64 {
        a * self.weight_a as f64 + b * self.weight_b as f64
    }
}

impl Stage for WeightedAverage {
    type Input = (f32, f32);
    type Output = f32;

    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(self.fuse_f32(input.0, input.1))
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_buffer_basic() {
        let mut buffer: TimestampBuffer<u32, 4> = TimestampBuffer::new();

        buffer.push(Timestamped::new(1, 100, 0));
        buffer.push(Timestamped::new(2, 200, 1));
        buffer.push(Timestamped::new(3, 300, 2));

        assert_eq!(buffer.len(), 3);

        // Find closest to 190 within tolerance 50 - only 200 is within range
        let closest = buffer.find_closest(190, 50);
        assert!(closest.is_some());
        assert_eq!(closest.unwrap().data, 2); // timestamp 200 is closest to 190
    }

    #[test]
    fn test_timestamp_buffer_prune() {
        let mut buffer: TimestampBuffer<u32, 4> = TimestampBuffer::new();

        buffer.push(Timestamped::new(1, 100, 0));
        buffer.push(Timestamped::new(2, 200, 1));
        buffer.push(Timestamped::new(3, 300, 2));

        buffer.prune_before(250);
        assert_eq!(buffer.len(), 1);

        let closest = buffer.find_closest(300, 10);
        assert!(closest.is_some());
        assert_eq!(closest.unwrap().data, 3);
    }

    #[test]
    fn test_timestamp_sync() {
        let mut sync: TimestampSync<u32, u32, 8> = TimestampSync::with_tolerance_us(100);

        // Push secondary data
        sync.push_secondary(Timestamped::new(10, 1000, 0));
        sync.push_secondary(Timestamped::new(20, 2000, 1));
        sync.push_secondary(Timestamped::new(30, 3000, 2));

        // Match primary
        let primary = Timestamped::new(100, 2050, 0);
        let pair = sync.match_primary(primary);

        assert!(pair.is_some());
        let p = pair.unwrap();
        assert_eq!(p.primary.data, 100);
        assert_eq!(p.secondary.data, 20); // Closest to timestamp 2050
    }

    #[test]
    fn test_timestamp_sync_no_match() {
        let mut sync: TimestampSync<u32, u32, 8> = TimestampSync::with_tolerance_us(10);

        sync.push_secondary(Timestamped::new(10, 1000, 0));

        // Primary is too far from secondary
        let primary = Timestamped::new(100, 1_000_000, 0);
        let pair = sync.match_primary(primary);

        assert!(pair.is_none());
        assert_eq!(sync.unmatched_count, 1);
    }

    #[test]
    fn test_timestamp_sync_as_stage() {
        let mut sync: TimestampSync<u32, u32, 8> = TimestampSync::with_tolerance_us(100);

        // Feed secondary data through stage interface
        assert!(sync.process(SyncInput::Secondary(Timestamped::new(10, 1000, 0))).is_none());
        assert!(sync.process(SyncInput::Secondary(Timestamped::new(20, 2000, 1))).is_none());

        // Feed primary data
        let result = sync.process(SyncInput::Primary(Timestamped::new(100, 1050, 0)));
        assert!(result.is_some());
        assert_eq!(result.unwrap().secondary.data, 10);
    }

    #[test]
    fn test_fuse_stage() {
        let mut stage = Fuse::new(|a: i32, b: i32| a + b);
        assert_eq!(stage.process((3, 5)), Some(8));
    }

    #[test]
    fn test_weighted_average() {
        let avg = WeightedAverage::new(3.0, 1.0);
        // 3:1 ratio means 75% of first, 25% of second
        let result = avg.fuse_f32(100.0, 0.0);
        assert!((result - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_weighted_average_equal() {
        let avg = WeightedAverage::equal();
        let result = avg.fuse_f32(10.0, 20.0);
        assert!((result - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_synced_pair() {
        let primary = Timestamped::new(1, 1000, 0);
        let secondary = Timestamped::new(2, 1100, 0);
        let pair = SyncedPair::new(primary, secondary);

        assert_eq!(pair.timestamp_ns(), 1000);
        assert_eq!(pair.time_diff_ns, 100);
    }
}
