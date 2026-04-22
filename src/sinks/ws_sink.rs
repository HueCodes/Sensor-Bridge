//! WebSocket broadcast sink.
//!
//! A [`WsSink`] binds a TCP listener, accepts WebSocket connections on
//! any path, and broadcasts each serialized item to every currently
//! connected client. Intended for dashboards — not for reliable
//! delivery. Slow clients have their send buffer overwritten via the
//! tokio broadcast channel's lagging behavior.

use std::net::SocketAddr;

use futures_util::SinkExt;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use crate::error::{PipelineError, Result};

use super::Sink;

/// WebSocket broadcast sink.
///
/// Call [`WsSink::bind`] from a tokio context to get a handle; drop the
/// handle to stop accepting new connections. Existing clients are
/// closed when the broadcast channel is dropped.
pub struct WsSink {
    tx: broadcast::Sender<String>,
    local_addr: SocketAddr,
    task: Option<JoinHandle<()>>,
}

impl WsSink {
    /// Binds a TCP listener and spawns the accept loop on the current
    /// tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the listener cannot be bound.
    pub async fn bind(addr: SocketAddr, broadcast_capacity: usize) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let (tx, _rx) = broadcast::channel::<String>(broadcast_capacity.max(1));
        let accept_tx = tx.clone();
        let task = tokio::spawn(async move {
            accept_loop(listener, accept_tx).await;
        });
        Ok(Self {
            tx,
            local_addr,
            task: Some(task),
        })
    }

    /// Returns the address the listener is bound to.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Number of currently connected clients, as seen by the broadcast
    /// channel.
    #[must_use]
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Drop for WsSink {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl<T: Serialize + Send> Sink<T> for WsSink {
    fn write(&mut self, item: T) -> Result<()> {
        let payload = serde_json::to_string(&item)
            .map_err(|_| PipelineError::StageError("ws sink serialize error"))?;
        // Fine if there are no subscribers — the dashboard just hasn't
        // connected yet.
        let _ = self.tx.send(payload);
        Ok(())
    }
}

async fn accept_loop(listener: TcpListener, tx: broadcast::Sender<String>) {
    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        let rx = tx.subscribe();
        tokio::spawn(async move {
            handle_client(stream, rx).await;
        });
    }
}

async fn handle_client(stream: tokio::net::TcpStream, mut rx: broadcast::Receiver<String>) {
    let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    while let Ok(msg) = rx.recv().await {
        if ws.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}
