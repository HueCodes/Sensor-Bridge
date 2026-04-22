//! Network-backed sensor drivers.
//!
//! These implement the crate's [`Sensor`] trait on top of async network
//! sockets. A tokio task runs in the background, parses incoming packets and
//! pushes them through a bounded channel; [`Sensor::sample`] drains that
//! channel without blocking.
//!
//! The only "sensors" living here are ones that are useful when no hardware
//! is attached to the host — UDP and framed TCP. Hardware-specific drivers
//! (I2C IMUs, 1-wire thermometers, etc.) are deliberately out of scope for
//! this crate to keep the dependency tree clean on every platform.
//!
//! [`Sensor`]: crate::sensor::Sensor

mod metrics;
mod tcp_receiver;
mod udp_receiver;

pub use metrics::{NetworkMetrics, NetworkMetricsSnapshot};
pub use tcp_receiver::{TcpConfig, TcpSensor};
pub use udp_receiver::{UdpConfig, UdpSensor};
