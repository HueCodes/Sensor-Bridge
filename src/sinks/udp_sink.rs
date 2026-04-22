//! UDP publisher sink.
//!
//! Serializes each item as JSON and fires it as a single datagram.
//! Built on blocking [`std::net::UdpSocket`] rather than tokio so the
//! sink is usable from the synchronous output side of the pipeline.

use std::net::{SocketAddr, UdpSocket};

use serde::Serialize;

use crate::error::{PipelineError, Result};

use super::Sink;

/// Blocking UDP JSON publisher.
///
/// The socket is bound once to an ephemeral local address; every
/// [`Sink::write`] performs one `send_to` against the peer. Best-effort
/// by design: a dropped datagram returns an error but the sink stays
/// usable.
pub struct UdpSink {
    socket: UdpSocket,
    peer: SocketAddr,
}

impl UdpSink {
    /// Binds an ephemeral UDP socket and targets `peer` for all writes.
    ///
    /// # Errors
    ///
    /// Returns an error if the local socket cannot be bound.
    pub fn new(peer: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(io)?;
        Ok(Self { socket, peer })
    }

    /// Returns the peer address this sink publishes to.
    #[must_use]
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// Returns the local address the sink is bound to.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the OS can't report the local address.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr().map_err(io)
    }
}

impl<T: Serialize + Send> Sink<T> for UdpSink {
    fn write(&mut self, item: T) -> Result<()> {
        let bytes = serde_json::to_vec(&item)
            .map_err(|_| PipelineError::StageError("udp sink serialize error"))?;
        self.socket.send_to(&bytes, self.peer).map_err(io)?;
        Ok(())
    }
}

fn io(_e: std::io::Error) -> PipelineError {
    PipelineError::StageError("udp sink io error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket as StdUdpSocket;
    use std::time::Duration;

    #[derive(serde::Serialize)]
    struct Msg {
        v: u32,
    }

    #[test]
    fn roundtrip_one_datagram() {
        let rx = StdUdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut sink = UdpSink::new(rx.local_addr().unwrap()).unwrap();
        sink.write(Msg { v: 7 }).unwrap();

        let mut buf = [0u8; 64];
        let (n, _) = rx.recv_from(&mut buf).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&buf[..n]).unwrap();
        assert_eq!(payload["v"], 7);
    }
}
