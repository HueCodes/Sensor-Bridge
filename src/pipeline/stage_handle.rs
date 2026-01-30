//! Per-stage thread management and control.
//!
//! This module provides `StageHandle` for managing individual pipeline stages,
//! including thread lifecycle, commands, and metrics collection.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::channel::{Receiver, Sender};
use crate::stage::Stage;

/// Commands that can be sent to a stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StageCommand {
    /// Continue normal processing.
    Run = 0,
    /// Pause processing (stop consuming input).
    Pause = 1,
    /// Resume processing after pause.
    Resume = 2,
    /// Reset stage state.
    Reset = 3,
    /// Shutdown the stage gracefully.
    Shutdown = 4,
}

impl From<u8> for StageCommand {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Run,
            1 => Self::Pause,
            2 => Self::Resume,
            3 => Self::Reset,
            4 => Self::Shutdown,
            _ => Self::Run,
        }
    }
}

/// State of a stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StageState {
    /// Stage is created but not started.
    Created = 0,
    /// Stage is starting up.
    Starting = 1,
    /// Stage is running normally.
    Running = 2,
    /// Stage is paused.
    Paused = 3,
    /// Stage is shutting down.
    Stopping = 4,
    /// Stage has stopped.
    Stopped = 5,
    /// Stage encountered an error.
    Error = 6,
}

impl From<u8> for StageState {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Created,
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::Paused,
            4 => Self::Stopping,
            5 => Self::Stopped,
            6 => Self::Error,
            _ => Self::Created,
        }
    }
}

/// Shared control state for a stage.
#[derive(Debug)]
pub struct StageControl {
    /// Current command for the stage.
    command: AtomicU8,
    /// Current state of the stage.
    state: AtomicU8,
    /// Whether shutdown has been requested.
    shutdown: AtomicBool,
    /// Whether the stage is paused.
    paused: AtomicBool,
}

impl StageControl {
    /// Creates new stage control.
    #[must_use]
    pub fn new() -> Self {
        Self {
            command: AtomicU8::new(StageCommand::Run as u8),
            state: AtomicU8::new(StageState::Created as u8),
            shutdown: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        }
    }

    /// Returns the current command.
    #[inline]
    pub fn command(&self) -> StageCommand {
        StageCommand::from(self.command.load(Ordering::Acquire))
    }

    /// Sets a new command.
    #[inline]
    pub fn set_command(&self, cmd: StageCommand) {
        self.command.store(cmd as u8, Ordering::Release);
        match cmd {
            StageCommand::Shutdown => self.shutdown.store(true, Ordering::Release),
            StageCommand::Pause => self.paused.store(true, Ordering::Release),
            StageCommand::Resume | StageCommand::Run => {
                self.paused.store(false, Ordering::Release)
            }
            StageCommand::Reset => {}
        }
    }

    /// Returns the current state.
    #[inline]
    pub fn state(&self) -> StageState {
        StageState::from(self.state.load(Ordering::Acquire))
    }

    /// Sets the state.
    #[inline]
    pub fn set_state(&self, state: StageState) {
        self.state.store(state as u8, Ordering::Release);
    }

    /// Returns whether shutdown has been requested.
    #[inline]
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    /// Returns whether the stage is paused.
    #[inline]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    /// Requests shutdown.
    #[inline]
    pub fn request_shutdown(&self) {
        self.set_command(StageCommand::Shutdown);
    }
}

impl Default for StageControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for managing a pipeline stage running in a dedicated thread.
///
/// This handle provides control over the stage lifecycle and access to
/// its metrics.
pub struct StageHandle<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// Name of the stage.
    name: String,
    /// Thread handle (None if not started or already joined).
    thread: Option<JoinHandle<()>>,
    /// Shared control state.
    control: Arc<StageControl>,
    /// Input channel sender (for feeding data to this stage).
    input_tx: Option<Sender<I>>,
    /// Output channel receiver (for consuming data from this stage).
    output_rx: Option<Receiver<O>>,
    /// Metrics for this stage.
    metrics: Arc<super::executor::StageMetrics>,
}

impl<I, O> StageHandle<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// Creates a new stage handle.
    pub(crate) fn new(
        name: impl Into<String>,
        thread: JoinHandle<()>,
        control: Arc<StageControl>,
        input_tx: Sender<I>,
        output_rx: Receiver<O>,
        metrics: Arc<super::executor::StageMetrics>,
    ) -> Self {
        Self {
            name: name.into(),
            thread: Some(thread),
            control,
            input_tx: Some(input_tx),
            output_rx: Some(output_rx),
            metrics,
        }
    }

    /// Returns the stage name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the current state of the stage.
    #[must_use]
    pub fn state(&self) -> StageState {
        self.control.state()
    }

    /// Returns whether the stage is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self.state(), StageState::Running)
    }

    /// Returns whether the stage is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.control.is_paused()
    }

    /// Pauses the stage.
    pub fn pause(&self) {
        self.control.set_command(StageCommand::Pause);
    }

    /// Resumes the stage.
    pub fn resume(&self) {
        self.control.set_command(StageCommand::Resume);
    }

    /// Resets the stage state.
    pub fn reset(&self) {
        self.control.set_command(StageCommand::Reset);
    }

    /// Requests the stage to shut down gracefully.
    pub fn shutdown(&self) {
        self.control.request_shutdown();
    }

    /// Returns a reference to the input sender.
    ///
    /// Use this to send data to the stage.
    #[must_use]
    pub fn input(&self) -> Option<&Sender<I>> {
        self.input_tx.as_ref()
    }

    /// Takes the input sender, transferring ownership.
    pub fn take_input(&mut self) -> Option<Sender<I>> {
        self.input_tx.take()
    }

    /// Returns a reference to the output receiver.
    ///
    /// Use this to receive data from the stage.
    #[must_use]
    pub fn output(&self) -> Option<&Receiver<O>> {
        self.output_rx.as_ref()
    }

    /// Takes the output receiver, transferring ownership.
    pub fn take_output(&mut self) -> Option<Receiver<O>> {
        self.output_rx.take()
    }

    /// Returns a reference to the stage metrics.
    #[must_use]
    pub fn metrics(&self) -> &super::executor::StageMetrics {
        &self.metrics
    }

    /// Waits for the stage thread to finish.
    ///
    /// Returns `Ok(())` if the thread finished normally, or `Err` if
    /// the thread panicked.
    pub fn join(&mut self) -> Result<(), Box<dyn std::any::Any + Send>> {
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|e| e)
        } else {
            Ok(())
        }
    }

    /// Returns whether the stage thread has finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().map_or(true, |t| t.is_finished())
    }
}

impl<I, O> Drop for StageHandle<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    fn drop(&mut self) {
        // Request shutdown if not already done
        self.shutdown();

        // Drop the input channel to signal EOF
        self.input_tx.take();

        // Wait for the thread to finish
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Builder for creating stage handles.
pub struct StageHandleBuilder<S>
where
    S: Stage + Send + 'static,
    S::Input: Send,
    S::Output: Send,
{
    name: String,
    stage: S,
    channel_capacity: usize,
}

impl<S> StageHandleBuilder<S>
where
    S: Stage + Send + 'static,
    S::Input: Send,
    S::Output: Send,
{
    /// Creates a new builder with the given stage.
    pub fn new(name: impl Into<String>, stage: S) -> Self {
        Self {
            name: name.into(),
            stage,
            channel_capacity: 1024,
        }
    }

    /// Sets the channel capacity for input/output channels.
    #[must_use]
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Builds and spawns the stage in a new thread.
    ///
    /// Returns a handle to control the stage.
    pub fn spawn(self) -> StageHandle<S::Input, S::Output> {
        use super::executor::{stage_worker, StageMetrics};

        let control = Arc::new(StageControl::new());
        let metrics = Arc::new(StageMetrics::new());

        let (input_tx, input_rx) = crate::channel::bounded(self.channel_capacity);
        let (output_tx, output_rx) = crate::channel::bounded(self.channel_capacity);

        let stage = self.stage;
        let name = self.name.clone();
        let control_clone = Arc::clone(&control);
        let metrics_clone = Arc::clone(&metrics);

        let thread = thread::Builder::new()
            .name(name.clone())
            .spawn(move || {
                stage_worker(stage, input_rx, output_tx, control_clone, metrics_clone);
            })
            .expect("failed to spawn stage thread");

        control.set_state(StageState::Running);

        StageHandle::new(name, thread, control, input_tx, output_rx, metrics)
    }

    /// Builds and spawns the stage, connecting it to an existing input channel.
    pub fn spawn_with_input(
        self,
        input_rx: Receiver<S::Input>,
    ) -> (StageHandle<S::Input, S::Output>, Receiver<S::Output>) {
        use super::executor::{stage_worker, StageMetrics};

        let control = Arc::new(StageControl::new());
        let metrics = Arc::new(StageMetrics::new());

        let (input_tx, _) = crate::channel::bounded(1); // Dummy, won't be used
        let (output_tx, output_rx) = crate::channel::bounded(self.channel_capacity);

        let stage = self.stage;
        let name = self.name.clone();
        let control_clone = Arc::clone(&control);
        let metrics_clone = Arc::clone(&metrics);

        let thread = thread::Builder::new()
            .name(name.clone())
            .spawn(move || {
                stage_worker(stage, input_rx, output_tx, control_clone, metrics_clone);
            })
            .expect("failed to spawn stage thread");

        control.set_state(StageState::Running);

        let output_rx_clone = output_rx.clone();
        let handle = StageHandle {
            name,
            thread: Some(thread),
            control,
            input_tx: Some(input_tx),
            output_rx: Some(output_rx),
            metrics,
        };

        (handle, output_rx_clone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::Map;
    use std::time::Duration;

    #[test]
    fn test_stage_control() {
        let control = StageControl::new();

        assert_eq!(control.state(), StageState::Created);
        assert_eq!(control.command(), StageCommand::Run);
        assert!(!control.is_shutdown());
        assert!(!control.is_paused());

        control.set_state(StageState::Running);
        assert_eq!(control.state(), StageState::Running);

        control.set_command(StageCommand::Pause);
        assert!(control.is_paused());

        control.set_command(StageCommand::Resume);
        assert!(!control.is_paused());

        control.request_shutdown();
        assert!(control.is_shutdown());
    }

    #[test]
    fn test_stage_handle_basic() {
        let stage = Map::new(|x: i32| x * 2);
        let mut handle = StageHandleBuilder::new("doubler", stage)
            .channel_capacity(16)
            .spawn();

        assert_eq!(handle.name(), "doubler");
        assert!(handle.is_running());

        // Send some data
        if let Some(input) = handle.input() {
            input.send(5).unwrap();
            input.send(10).unwrap();
        }

        // Receive results
        std::thread::sleep(Duration::from_millis(10));

        if let Some(output) = handle.output() {
            let r1 = output.try_recv().ok();
            let r2 = output.try_recv().ok();
            assert_eq!(r1, Some(10));
            assert_eq!(r2, Some(20));
        }

        // Shutdown
        handle.shutdown();
        handle.join().unwrap();
        assert!(handle.is_finished());
    }

    #[test]
    fn test_stage_handle_metrics() {
        let stage = Map::new(|x: i32| x * 2);
        let mut handle = StageHandleBuilder::new("test", stage).spawn();

        if let Some(input) = handle.input() {
            for i in 0..100 {
                input.send(i).unwrap();
            }
        }

        std::thread::sleep(Duration::from_millis(50));

        let metrics = handle.metrics();
        assert!(metrics.input_count() > 0);

        handle.shutdown();
        handle.join().unwrap();
    }

    #[test]
    fn test_stage_handle_pause_resume() {
        let stage = Map::new(|x: i32| x * 2);
        let handle = StageHandleBuilder::new("test", stage).spawn();

        handle.pause();
        assert!(handle.is_paused());

        handle.resume();
        assert!(!handle.is_paused());

        handle.shutdown();
    }
}
