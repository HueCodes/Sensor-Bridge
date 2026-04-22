//! UDP receiver that adapts a socket into a [`Sensor`].
//!
//! [`UdpSensor`] binds a UDP socket on a tokio runtime task, deserializes
//! each incoming datagram as JSON into `T`, and hands the decoded value to
//! [`Sensor::sample`] through a bounded channel. Parse failures and queue
//! overflows are tracked in [`NetworkMetrics`] instead of killing the task —
//! the network is an unreliable medium and a single bad packet must not
//! take the pipeline down.

use std::net::SocketAddr;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::error::{PipelineError, Result, SensorError};
use crate::sensor::Sensor;
use crate::timestamp::{MonotonicClock, Timestamped};

use super::metrics::NetworkMetrics;

/// Configuration for a [`UdpSensor`].
#[derive(Debug, Clone)]
pub struct UdpConfig {
    /// Address to bind the UDP socket to.
    pub bind_addr: SocketAddr,
    /// Human-readable sensor name surfaced through [`Sensor::name`].
    pub name: &'static str,
    /// Advertised sample rate in Hz (informational; UDP is event-driven).
    pub sample_rate_hz: f64,
    /// Maximum number of decoded readings queued before the receiver task
    /// starts dropping the oldest to keep latency bounded.
    pub queue_capacity: usize,
    /// Maximum datagram size the socket will accept.
    pub max_packet_bytes: usize,
}

impl UdpConfig {
    /// Returns a new config bound to the given address, with sensible
    /// defaults for the remaining knobs.
    #[must_use]
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            name: "udp_sensor",
            sample_rate_hz: 0.0,
            queue_capacity: 1024,
            max_packet_bytes: 2048,
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

/// A sensor backed by line-delimited JSON datagrams on a UDP socket.
///
/// `T` is the deserialized payload type and must implement
/// [`DeserializeOwned`]. The background task is spawned onto the current
/// tokio runtime when you call [`UdpSensor::spawn`]; the returned handle
/// is `Send` and can be fed directly into the crate's pipeline builders.
///
/// # Example
///
/// ```rust,no_run
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// use sensor_bridge::drivers::{UdpConfig, UdpSensor};
/// use serde::Deserialize;
///
/// #[derive(Debug, Clone, Deserialize)]
/// struct Packet { value: f32 }
///
/// let cfg = UdpConfig::new("0.0.0.0:9000".parse()?).with_name("demo");
/// let sensor: UdpSensor<Packet> = UdpSensor::spawn(cfg).await?;
/// # Ok(()) }
/// ```
pub struct UdpSensor<T: Send + 'static> {
    name: &'static str,
    sample_rate_hz: f64,
    rx: mpsc::Receiver<Timestamped<T>>,
    metrics: Arc<NetworkMetrics>,
    local_addr: SocketAddr,
    task: Option<JoinHandle<()>>,
}

impl<T: DeserializeOwned + Send + 'static> UdpSensor<T> {
    /// Binds a UDP socket and spawns the receiver task on the current
    /// tokio runtime. Returns once the socket is ready.
    pub async fn spawn(config: UdpConfig) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(config.bind_addr).await?;
        let local_addr = socket.local_addr()?;
        let (tx, rx) = mpsc::channel(config.queue_capacity);
        let metrics = NetworkMetrics::new();
        let task = tokio::spawn(run(
            socket,
            tx,
            Arc::clone(&metrics),
            config.max_packet_bytes,
        ));

        Ok(Self {
            name: config.name,
            sample_rate_hz: config.sample_rate_hz,
            rx,
            metrics,
            local_addr,
            task: Some(task),
        })
    }

    /// Address the socket actually bound to. Useful when the config
    /// specified port 0.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Returns a handle to the shared metric counters.
    #[must_use]
    pub fn metrics(&self) -> Arc<NetworkMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Awaits the next decoded reading, yielding when the channel is empty.
    ///
    /// Prefer [`Sensor::sample`] from synchronous code. This method is
    /// provided for callers that are already in async context and want to
    /// drive the sensor directly.
    pub async fn recv(&mut self) -> Option<Timestamped<T>> {
        self.rx.recv().await
    }
}

impl<T: Send + 'static> Drop for UdpSensor<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl<T: DeserializeOwned + Send + 'static> Sensor for UdpSensor<T> {
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
    socket: UdpSocket,
    tx: mpsc::Sender<Timestamped<T>>,
    metrics: Arc<NetworkMetrics>,
    max_packet_bytes: usize,
) {
    let clock = MonotonicClock::new();
    let mut buf = vec![0u8; max_packet_bytes];

    loop {
        let len = match socket.recv(&mut buf).await {
            Ok(len) => len,
            Err(_) => {
                metrics.socket_errors.increment();
                continue;
            }
        };

        let payload = &buf[..len];
        match serde_json::from_slice::<T>(payload) {
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
