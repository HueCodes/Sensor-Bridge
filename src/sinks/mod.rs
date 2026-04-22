//! Output sinks for processed pipeline data.
//!
//! A sink consumes the tail of a pipeline — persistence, publishing,
//! telemetry — and is always the last thing in the chain. Each sink in
//! this module exposes a small, blocking `Sink` trait implemented
//! synchronously; async sinks (WebSocket, UDP from a tokio runtime)
//! expose their own async methods on top of that.
//!
//! Sinks here are deliberately simple: they do one job, in one format,
//! with a minimum of configuration knobs. Wrap them in a [`Stage`] if
//! you want to tee a pipeline into multiple sinks simultaneously.
//!
//! [`Stage`]: crate::stage::Stage

mod csv_sink;
mod tap_sink;

#[cfg(feature = "network")]
mod udp_sink;

#[cfg(feature = "websocket")]
mod ws_sink;

pub use csv_sink::{CsvRow, CsvSink};
pub use tap_sink::TapSink;

#[cfg(feature = "network")]
pub use udp_sink::UdpSink;

#[cfg(feature = "websocket")]
pub use ws_sink::WsSink;

use crate::error::Result;

/// A blocking consumer at the tail of a pipeline.
///
/// Implementations should be best-effort on the hot path: a slow sink
/// must not block the pipeline's terminal stage for long. Where a sink
/// needs durability (CSV flush, WS send buffer), document the tradeoff.
pub trait Sink<T>: Send {
    /// Hand a single item to the sink.
    ///
    /// # Errors
    ///
    /// Returns a [`PipelineError`] if the sink cannot accept the item —
    /// typically an IO failure. Callers decide whether to retry, drop or
    /// halt.
    ///
    /// [`PipelineError`]: crate::error::PipelineError
    fn write(&mut self, item: T) -> Result<()>;

    /// Flush any internal buffers. Default is a no-op.
    ///
    /// # Errors
    ///
    /// Returns a [`PipelineError`] if flushing fails.
    ///
    /// [`PipelineError`]: crate::error::PipelineError
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    /// Called once when the pipeline is shutting down.
    fn close(&mut self) -> Result<()> {
        self.flush()
    }
}
