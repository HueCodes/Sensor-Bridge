//! TCP receiver with length-prefixed framing and auto-reconnect.
//!
//! Wire format: `u32` little-endian length header, followed by that many
//! bytes of JSON payload. Any read error triggers a reconnect with capped
//! exponential backoff; the receiver task only exits when the sensor
//! handle is dropped.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::error::{PipelineError, Result, SensorError};
use crate::sensor::Sensor;
use crate::timestamp::{MonotonicClock, Timestamped};

use super::metrics::NetworkMetrics;

/// Configuration for a [`TcpSensor`].
#[derive(Debug, Clone)]
pub struct TcpConfig {
    /// Peer address to connect to.
    pub peer_addr: SocketAddr,
    /// Human-readable sensor name.
    pub name: &'static str,
    /// Advertised sample rate in Hz.
    pub sample_rate_hz: f64,
    /// Capacity of the decoded-reading queue.
    pub queue_capacity: usize,
    /// Maximum accepted frame payload size. Frames larger than this close
    /// the connection so we don't try to allocate arbitrarily.
    pub max_frame_bytes: usize,
    /// First reconnect delay. Doubles on each consecutive failure up to
    /// `max_backoff`.
    pub initial_backoff: Duration,
    /// Upper bound on the reconnect delay.
    pub max_backoff: Duration,
}

impl TcpConfig {
    /// Returns a new config targeting `peer_addr` with sensible defaults.
    #[must_use]
    pub fn new(peer_addr: SocketAddr) -> Self {
        Self {
            peer_addr,
            name: "tcp_sensor",
            sample_rate_hz: 0.0,
            queue_capacity: 1024,
            max_frame_bytes: 64 * 1024,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
        }
    }

    /// Sets the human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    /// Sets the advertised sample rate.
    #[must_use]
    pub fn with_sample_rate(mut self, hz: f64) -> Self {
        self.sample_rate_hz = hz;
        self
    }

    /// Sets the queue capacity.
    #[must_use]
    pub fn with_queue_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = capacity;
        self
    }
}

/// A sensor backed by length-prefixed JSON frames on a TCP stream.
///
/// The background task reconnects indefinitely. Callers observing
/// [`NetworkMetrics::reconnects`] can alert when a peer is flapping.
pub struct TcpSensor<T: Send + 'static> {
    name: &'static str,
    sample_rate_hz: f64,
    rx: mpsc::Receiver<Timestamped<T>>,
    metrics: Arc<NetworkMetrics>,
    task: Option<JoinHandle<()>>,
}

impl<T: DeserializeOwned + Send + 'static> TcpSensor<T> {
    /// Spawns the receiver task on the current tokio runtime and returns
    /// immediately — the initial connection attempt happens in the
    /// background so construction never blocks.
    pub fn spawn(config: TcpConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.queue_capacity);
        let metrics = NetworkMetrics::new();
        let task = tokio::spawn(run(config.clone(), tx, Arc::clone(&metrics)));

        Self {
            name: config.name,
            sample_rate_hz: config.sample_rate_hz,
            rx,
            metrics,
            task: Some(task),
        }
    }

    /// Returns a handle to the shared metric counters.
    #[must_use]
    pub fn metrics(&self) -> Arc<NetworkMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Awaits the next decoded frame, yielding when the channel is empty.
    pub async fn recv(&mut self) -> Option<Timestamped<T>> {
        self.rx.recv().await
    }
}

impl<T: Send + 'static> Drop for TcpSensor<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl<T: DeserializeOwned + Send + 'static> Sensor for TcpSensor<T> {
    type Reading = T;

    fn sample(&mut self) -> Result<Timestamped<Self::Reading>> {
        match self.rx.try_recv() {
            Ok(reading) => Ok(reading),
            Err(mpsc::error::TryRecvError::Empty) => {
                Err(PipelineError::SensorError(SensorError::Timeout))
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(PipelineError::SensorError(SensorError::Disconnected))
            }
        }
    }

    fn name(&self) -> &str {
        self.name
    }

    fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }
}

async fn run<T: DeserializeOwned + Send + 'static>(
    config: TcpConfig,
    tx: mpsc::Sender<Timestamped<T>>,
    metrics: Arc<NetworkMetrics>,
) {
    let clock = MonotonicClock::new();
    let mut backoff = config.initial_backoff;

    loop {
        if tx.is_closed() {
            return;
        }

        let stream = match TcpStream::connect(config.peer_addr).await {
            Ok(s) => {
                backoff = config.initial_backoff;
                s
            }
            Err(_) => {
                metrics.socket_errors.increment();
                metrics.reconnects.increment();
                sleep(backoff).await;
                backoff = (backoff * 2).min(config.max_backoff);
                continue;
            }
        };

        handle_stream(stream, &tx, &metrics, &clock, config.max_frame_bytes).await;
        metrics.reconnects.increment();
        sleep(config.initial_backoff).await;
    }
}

async fn handle_stream<T: DeserializeOwned + Send + 'static>(
    mut stream: TcpStream,
    tx: &mpsc::Sender<Timestamped<T>>,
    metrics: &NetworkMetrics,
    clock: &MonotonicClock,
    max_frame_bytes: usize,
) {
    let mut len_buf = [0u8; 4];

    loop {
        if tx.is_closed() {
            return;
        }

        if let Err(_e) = stream.read_exact(&mut len_buf).await {
            metrics.socket_errors.increment();
            return;
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 || len > max_frame_bytes {
            metrics.parse_errors.increment();
            return;
        }

        let mut payload = vec![0u8; len];
        if let Err(_e) = stream.read_exact(&mut payload).await {
            metrics.socket_errors.increment();
            return;
        }

        match serde_json::from_slice::<T>(&payload) {
            Ok(value) => {
                metrics.packets_received.increment();
                let stamped = clock.stamp(value);
                if tx.try_send(stamped).is_err() {
                    metrics.packets_dropped.increment();
                }
            }
            Err(_) => {
                metrics.parse_errors.increment();
            }
        }
    }
}
