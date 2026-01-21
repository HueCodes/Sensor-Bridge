//! Pipeline stage traits and combinators.
//!
//! A pipeline stage processes data, transforming inputs to outputs.
//! Stages can be composed using combinators like [`Chain`] and [`Branch`].
//!
//! # Stage Types
//!
//! - **Filter**: Removes or passes through data (1:0 or 1:1)
//! - **Transform**: Converts data from one type to another (1:1)
//! - **Aggregate**: Combines multiple inputs into one output (N:1)
//! - **Split**: Produces multiple outputs from one input (1:N)

use core::marker::PhantomData;

/// A processing stage in the sensor pipeline.
///
/// Each stage transforms an input into zero or more outputs. The [`process`](Self::process)
/// method may return `None` to indicate filtering (dropped sample) or buffering
/// (waiting for more data).
///
/// # Example
///
/// ```rust
/// use sensor_pipeline::stage::Stage;
///
/// struct Doubler;
///
/// impl Stage for Doubler {
///     type Input = i32;
///     type Output = i32;
///
///     fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
///         Some(input * 2)
///     }
///
///     fn reset(&mut self) {}
/// }
///
/// let mut stage = Doubler;
/// assert_eq!(stage.process(5), Some(10));
/// ```
pub trait Stage: Send {
    /// The type of data this stage accepts.
    type Input;
    /// The type of data this stage produces.
    type Output;

    /// Processes one input, optionally producing output.
    ///
    /// Returns `None` if:
    /// - The input was filtered out
    /// - The stage is buffering (e.g., waiting for window to fill)
    /// - The stage is aggregating multiple inputs
    fn process(&mut self, input: Self::Input) -> Option<Self::Output>;

    /// Called periodically when no input is available.
    ///
    /// This allows stages to produce output based on time (e.g., timeout,
    /// periodic flush) even when no new data arrives.
    ///
    /// Default implementation returns `None`.
    fn tick(&mut self) -> Option<Self::Output> {
        None
    }

    /// Resets the stage to its initial state.
    ///
    /// This clears any internal buffers, accumulators, or state.
    fn reset(&mut self);

    /// Flushes any buffered data, producing final output(s).
    ///
    /// Called when the pipeline is shutting down to ensure no data is lost.
    /// Default implementation calls `tick()`.
    fn flush(&mut self) -> Option<Self::Output> {
        self.tick()
    }
}

/// Chains two stages together, passing the output of the first to the second.
///
/// The [`Chain`] combinator creates a pipeline where data flows through
/// `S1` first, and if it produces output, that output is passed to `S2`.
///
/// # Example
///
/// ```rust
/// use sensor_pipeline::stage::{Stage, Chain, Map};
///
/// let double = Map::new(|x: i32| x * 2);
/// let add_one = Map::new(|x: i32| x + 1);
/// let mut pipeline = Chain::new(double, add_one);
///
/// assert_eq!(pipeline.process(5), Some(11)); // (5 * 2) + 1
/// ```
pub struct Chain<S1, S2> {
    first: S1,
    second: S2,
}

impl<S1, S2> Chain<S1, S2> {
    /// Creates a new chain of two stages.
    #[inline]
    #[must_use]
    pub const fn new(first: S1, second: S2) -> Self {
        Self { first, second }
    }

    /// Returns a reference to the first stage.
    #[inline]
    #[must_use]
    pub const fn first(&self) -> &S1 {
        &self.first
    }

    /// Returns a reference to the second stage.
    #[inline]
    #[must_use]
    pub const fn second(&self) -> &S2 {
        &self.second
    }

    /// Returns mutable references to both stages.
    #[inline]
    pub fn stages_mut(&mut self) -> (&mut S1, &mut S2) {
        (&mut self.first, &mut self.second)
    }
}

impl<S1, S2> Stage for Chain<S1, S2>
where
    S1: Stage,
    S2: Stage<Input = S1::Output>,
{
    type Input = S1::Input;
    type Output = S2::Output;

    #[inline]
    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        self.first.process(input).and_then(|mid| self.second.process(mid))
    }

    fn tick(&mut self) -> Option<Self::Output> {
        // Try tick on first stage, pass to second if it produces output
        if let Some(mid) = self.first.tick() {
            if let Some(out) = self.second.process(mid) {
                return Some(out);
            }
        }
        // Also tick the second stage
        self.second.tick()
    }

    fn reset(&mut self) {
        self.first.reset();
        self.second.reset();
    }

    fn flush(&mut self) -> Option<Self::Output> {
        // Flush first stage and pass to second
        if let Some(mid) = self.first.flush() {
            if let Some(out) = self.second.process(mid) {
                return Some(out);
            }
        }
        self.second.flush()
    }
}

// Implement Send if both stages are Send
unsafe impl<S1: Send, S2: Send> Send for Chain<S1, S2> {}

/// A simple mapping stage that transforms inputs using a function.
///
/// This always produces output (1:1 transformation).
pub struct Map<I, O, F>
where
    F: FnMut(I) -> O,
{
    f: F,
    _phantom: PhantomData<fn(I) -> O>,
}

impl<I, O, F> Map<I, O, F>
where
    F: FnMut(I) -> O,
{
    /// Creates a new mapping stage.
    #[inline]
    #[must_use]
    pub const fn new(f: F) -> Self {
        Self {
            f,
            _phantom: PhantomData,
        }
    }
}

impl<I, O, F> Stage for Map<I, O, F>
where
    F: FnMut(I) -> O + Send,
    I: Send,
    O: Send,
{
    type Input = I;
    type Output = O;

    #[inline]
    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some((self.f)(input))
    }

    fn reset(&mut self) {}
}

/// A filtering stage that passes through inputs that match a predicate.
pub struct Filter<T, F>
where
    F: FnMut(&T) -> bool,
{
    predicate: F,
    _phantom: PhantomData<T>,
}

impl<T, F> Filter<T, F>
where
    F: FnMut(&T) -> bool,
{
    /// Creates a new filter stage.
    #[inline]
    #[must_use]
    pub const fn new(predicate: F) -> Self {
        Self {
            predicate,
            _phantom: PhantomData,
        }
    }
}

impl<T, F> Stage for Filter<T, F>
where
    F: FnMut(&T) -> bool + Send,
    T: Send,
{
    type Input = T;
    type Output = T;

    #[inline]
    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        if (self.predicate)(&input) {
            Some(input)
        } else {
            None
        }
    }

    fn reset(&mut self) {}
}

/// A stage that combines filtering and mapping.
pub struct FilterMap<I, O, F>
where
    F: FnMut(I) -> Option<O>,
{
    f: F,
    _phantom: PhantomData<fn(I) -> Option<O>>,
}

impl<I, O, F> FilterMap<I, O, F>
where
    F: FnMut(I) -> Option<O>,
{
    /// Creates a new filter-map stage.
    #[inline]
    #[must_use]
    pub const fn new(f: F) -> Self {
        Self {
            f,
            _phantom: PhantomData,
        }
    }
}

impl<I, O, F> Stage for FilterMap<I, O, F>
where
    F: FnMut(I) -> Option<O> + Send,
    I: Send,
    O: Send,
{
    type Input = I;
    type Output = O;

    #[inline]
    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        (self.f)(input)
    }

    fn reset(&mut self) {}
}

/// A pass-through stage that does nothing.
///
/// Useful as a placeholder or for type system purposes.
pub struct Identity<T>(PhantomData<T>);

impl<T> Identity<T> {
    /// Creates a new identity stage.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for Identity<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send> Stage for Identity<T> {
    type Input = T;
    type Output = T;

    #[inline]
    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        Some(input)
    }

    fn reset(&mut self) {}
}

/// A stage that inspects values without modifying them.
///
/// Useful for debugging, logging, or metrics.
pub struct Inspect<T, F>
where
    F: FnMut(&T),
{
    f: F,
    _phantom: PhantomData<T>,
}

impl<T, F> Inspect<T, F>
where
    F: FnMut(&T),
{
    /// Creates a new inspect stage.
    #[inline]
    #[must_use]
    pub const fn new(f: F) -> Self {
        Self {
            f,
            _phantom: PhantomData,
        }
    }
}

impl<T, F> Stage for Inspect<T, F>
where
    F: FnMut(&T) + Send,
    T: Send,
{
    type Input = T;
    type Output = T;

    #[inline]
    fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
        (self.f)(&input);
        Some(input)
    }

    fn reset(&mut self) {}
}

/// Extension trait for composing stages.
pub trait StageExt: Stage + Sized {
    /// Chains this stage with another.
    fn chain<S>(self, next: S) -> Chain<Self, S>
    where
        S: Stage<Input = Self::Output>,
    {
        Chain::new(self, next)
    }

    /// Maps the output of this stage.
    fn map<O, F>(self, f: F) -> Chain<Self, Map<Self::Output, O, F>>
    where
        F: FnMut(Self::Output) -> O + Send,
        O: Send,
        Self::Output: Send,
    {
        Chain::new(self, Map::new(f))
    }

    /// Filters the output of this stage.
    fn filter<F>(self, predicate: F) -> Chain<Self, Filter<Self::Output, F>>
    where
        F: FnMut(&Self::Output) -> bool + Send,
        Self::Output: Send,
    {
        Chain::new(self, Filter::new(predicate))
    }

    /// Inspects the output of this stage.
    fn inspect<F>(self, f: F) -> Chain<Self, Inspect<Self::Output, F>>
    where
        F: FnMut(&Self::Output) + Send,
        Self::Output: Send,
    {
        Chain::new(self, Inspect::new(f))
    }
}

// Blanket implementation for all stages
impl<S: Stage> StageExt for S {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_stage() {
        let mut stage = Map::new(|x: i32| x * 2);
        assert_eq!(stage.process(5), Some(10));
        assert_eq!(stage.process(0), Some(0));
        assert_eq!(stage.process(-3), Some(-6));
    }

    #[test]
    fn test_filter_stage() {
        let mut stage = Filter::new(|x: &i32| *x > 0);
        assert_eq!(stage.process(5), Some(5));
        assert_eq!(stage.process(0), None);
        assert_eq!(stage.process(-3), None);
    }

    #[test]
    fn test_filter_map_stage() {
        let mut stage = FilterMap::new(|x: i32| if x > 0 { Some(x * 2) } else { None });
        assert_eq!(stage.process(5), Some(10));
        assert_eq!(stage.process(0), None);
        assert_eq!(stage.process(-3), None);
    }

    #[test]
    fn test_identity_stage() {
        let mut stage: Identity<i32> = Identity::new();
        assert_eq!(stage.process(42), Some(42));
    }

    #[test]
    fn test_chain_stage() {
        let double = Map::new(|x: i32| x * 2);
        let add_one = Map::new(|x: i32| x + 1);
        let mut chain = Chain::new(double, add_one);

        assert_eq!(chain.process(5), Some(11)); // (5 * 2) + 1
    }

    #[test]
    fn test_chain_with_filter() {
        let filter = Filter::new(|x: &i32| *x > 0);
        let double = Map::new(|x: i32| x * 2);
        let mut chain = Chain::new(filter, double);

        assert_eq!(chain.process(5), Some(10));
        assert_eq!(chain.process(-5), None);
    }

    #[test]
    fn test_stage_ext() {
        let mut pipeline = Map::new(|x: i32| x * 2)
            .filter(|x| *x > 5)
            .map(|x| x + 1);

        assert_eq!(pipeline.process(1), None);   // 2, filtered
        assert_eq!(pipeline.process(3), Some(7)); // 6 + 1
        assert_eq!(pipeline.process(5), Some(11)); // 10 + 1
    }

    #[test]
    fn test_inspect_stage() {
        use std::sync::atomic::{AtomicI32, Ordering};

        let counter = AtomicI32::new(0);
        let mut stage = Inspect::new(|x: &i32| {
            counter.fetch_add(*x, Ordering::SeqCst);
        });

        stage.process(5);
        stage.process(3);
        assert_eq!(counter.load(Ordering::SeqCst), 8);
    }

    #[test]
    fn test_chain_reset() {
        struct Counter {
            count: i32,
        }

        impl Stage for Counter {
            type Input = i32;
            type Output = i32;

            fn process(&mut self, input: Self::Input) -> Option<Self::Output> {
                self.count += 1;
                Some(input + self.count)
            }

            fn reset(&mut self) {
                self.count = 0;
            }
        }

        let mut chain = Chain::new(Counter { count: 0 }, Counter { count: 0 });

        assert_eq!(chain.process(10), Some(12)); // 10 + 1 + 1
        assert_eq!(chain.process(10), Some(14)); // 10 + 2 + 2

        chain.reset();

        assert_eq!(chain.process(10), Some(12)); // Back to initial state
    }
}
