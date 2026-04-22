//! Counters shared by the network drivers.

use std::sync::Arc;

use crate::metrics::Counter;

/// Snapshot-friendly bundle of counters used by the UDP / TCP drivers.
///
/// All counters are lock-free and cheap to increment on the hot path. They
/// are intentionally kept in a single struct so a driver can share one
/// handle with whoever wants to observe it.
#[derive(Debug, Default)]
pub struct NetworkMetrics {
    /// Packets (or frames) successfully received from the socket.
    pub packets_received: Counter,
    /// Packets dropped because the internal queue was full.
    pub packets_dropped: Counter,
    /// Payloads that failed to deserialize.
    pub parse_errors: Counter,
    /// Socket-level errors reported by the OS.
    pub socket_errors: Counter,
    /// Reconnect attempts (TCP only).
    pub reconnects: Counter,
}

impl NetworkMetrics {
    /// Creates a zeroed metric bundle.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Returns a point-in-time read of every counter.
    #[must_use]
    pub fn snapshot(&self) -> NetworkMetricsSnapshot {
        NetworkMetricsSnapshot {
            packets_received: self.packets_received.get(),
            packets_dropped: self.packets_dropped.get(),
            parse_errors: self.parse_errors.get(),
            socket_errors: self.socket_errors.get(),
            reconnects: self.reconnects.get(),
        }
    }
}

/// Copy of [`NetworkMetrics`] with plain integers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetworkMetricsSnapshot {
    /// Successfully received packets/frames.
    pub packets_received: u64,
    /// Dropped due to full queue.
    pub packets_dropped: u64,
    /// Deserialization failures.
    pub parse_errors: u64,
    /// Socket-level errors.
    pub socket_errors: u64,
    /// Reconnect attempts (TCP).
    pub reconnects: u64,
}
