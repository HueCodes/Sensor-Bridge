//! Integration tests for the UDP/TCP drivers.
//!
//! These tests bind to an ephemeral port on localhost, drive a few packets
//! through the receiver, and assert the metrics reflect what we sent. They
//! all run under a single-threaded tokio runtime to stay portable across CI
//! hosts.

#![cfg(feature = "network")]

use std::net::SocketAddr;
use std::time::Duration;

use sensor_bridge::drivers::{TcpConfig, TcpSensor, UdpConfig, UdpSensor};
use sensor_bridge::sensor::Sensor;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::timeout;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Packet {
    seq: u32,
    value: f32,
}

fn loopback(port: u16) -> SocketAddr {
    format!("127.0.0.1:{port}").parse().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_receives_and_decodes_json() {
    let cfg = UdpConfig::new(loopback(0)).with_queue_capacity(16);
    let mut sensor: UdpSensor<Packet> = UdpSensor::spawn(cfg).await.unwrap();
    let target = sensor.local_addr();

    let client = UdpSocket::bind(loopback(0)).await.unwrap();
    for seq in 0..3u32 {
        let pkt = Packet {
            seq,
            value: seq as f32 * 1.5,
        };
        let bytes = serde_json::to_vec(&pkt).unwrap();
        client.send_to(&bytes, target).await.unwrap();
    }

    let mut received = Vec::new();
    for _ in 0..3 {
        let stamped = timeout(Duration::from_secs(2), sensor.recv())
            .await
            .expect("recv timed out")
            .expect("sensor closed");
        received.push(stamped.data);
    }

    assert_eq!(received.len(), 3);
    assert_eq!(received[0].seq, 0);
    let snap = sensor.metrics().snapshot();
    assert_eq!(snap.packets_received, 3);
    assert_eq!(snap.parse_errors, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_counts_parse_errors() {
    let cfg = UdpConfig::new(loopback(0)).with_queue_capacity(4);
    let sensor: UdpSensor<Packet> = UdpSensor::spawn(cfg).await.unwrap();
    let target = sensor.local_addr();
    let metrics = sensor.metrics();

    let client = UdpSocket::bind(loopback(0)).await.unwrap();
    client.send_to(b"not-json", target).await.unwrap();

    // Poll the metrics up to 2s; the background task is async so we don't
    // have a synchronous hook to wait on.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if metrics.parse_errors.get() >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(metrics.parse_errors.get() >= 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_sensor_trait_sample_drains_channel() {
    let cfg = UdpConfig::new(loopback(0));
    let mut sensor: UdpSensor<Packet> = UdpSensor::spawn(cfg).await.unwrap();
    let target = sensor.local_addr();

    let client = UdpSocket::bind(loopback(0)).await.unwrap();
    let pkt = Packet {
        seq: 42,
        value: 2.5,
    };
    client
        .send_to(&serde_json::to_vec(&pkt).unwrap(), target)
        .await
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut result = None;
    while std::time::Instant::now() < deadline {
        if let Ok(stamped) = sensor.sample() {
            result = Some(stamped.data);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(result, Some(pkt));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_frames_roundtrip_and_reconnect() {
    let listener = TcpListener::bind(loopback(0)).await.unwrap();
    let addr = listener.local_addr().unwrap();

    let cfg = TcpConfig::new(addr)
        .with_queue_capacity(8)
        .with_name("tcp_test");
    let mut sensor: TcpSensor<Packet> = TcpSensor::spawn(cfg);

    // First connection: send a frame, close abruptly.
    let (mut stream, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let pkt = Packet { seq: 1, value: 9.0 };
    let body = serde_json::to_vec(&pkt).unwrap();
    let header = (body.len() as u32).to_le_bytes();
    stream.write_all(&header).await.unwrap();
    stream.write_all(&body).await.unwrap();
    drop(stream);

    let first = timeout(Duration::from_secs(2), sensor.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.data, pkt);

    // Second connection after reconnect.
    let (mut stream, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let pkt2 = Packet {
        seq: 2,
        value: 11.0,
    };
    let body = serde_json::to_vec(&pkt2).unwrap();
    let header = (body.len() as u32).to_le_bytes();
    stream.write_all(&header).await.unwrap();
    stream.write_all(&body).await.unwrap();

    let second = timeout(Duration::from_secs(2), sensor.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.data, pkt2);

    let snap = sensor.metrics().snapshot();
    assert!(snap.packets_received >= 2);
    assert!(snap.reconnects >= 1);
}
