//! UDP end-to-end demo.
//!
//! Binds a UDP socket, decodes JSON sensor packets into the 4-stage pipeline,
//! then prints live per-stage throughput / latency / packet-loss stats. Pair
//! with `scripts/mock_udp_sender.py` for a zero-hardware demo; pair with an
//! ESP32 firmware publishing the same shape for the real-hardware version.
//!
//! Packet shape (line-delimited JSON datagrams):
//!
//! ```json
//! {"sensor":"mpu6050","ts_us":123456,"accel":[x,y,z],"gyro":[x,y,z]}
//! ```
//!
//! Run with:
//!
//! ```text
//! cargo run --features network --example udp_demo -- --bind 127.0.0.1:9000
//! ```
//!
//! In another shell:
//!
//! ```text
//! python3 scripts/mock_udp_sender.py --target 127.0.0.1:9000
//! ```

use std::env;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sensor_bridge::drivers::{UdpConfig, UdpSensor};
use sensor_bridge::pipeline::{MultiStagePipelineBuilder, PipelineConfig};
use sensor_bridge::stage::{Filter, Map};
use serde::Deserialize;

/// Payload shape from `scripts/mock_udp_sender.py`. Non-IMU sensors pass
/// through the filter stage untouched; extra fields are ignored.
#[derive(Debug, Clone, Deserialize)]
struct Packet {
    sensor: String,
    #[serde(default)]
    ts_us: u64,
    #[serde(default)]
    accel: Option<[f32; 3]>,
    #[serde(default)]
    gyro: Option<[f32; 3]>,
}

#[derive(Debug, Clone)]
struct ImuSample {
    ts_us: u64,
    gyro: [f32; 3],
    accel_magnitude: f32,
}

#[derive(Debug, Clone)]
struct FusedReading {
    ts_us: u64,
    accel_magnitude: f32,
    gyro_rate: f32,
}

fn parse_bind_addr() -> SocketAddr {
    let mut args = env::args().skip(1);
    let mut addr = "127.0.0.1:9000".to_string();
    while let Some(arg) = args.next() {
        if arg == "--bind" {
            if let Some(value) = args.next() {
                addr = value;
            }
        }
    }
    addr.parse().expect("invalid --bind address")
}

#[tokio::main]
async fn main() {
    let bind_addr = parse_bind_addr();

    println!("sensor-bridge UDP demo");
    println!("  binding to {bind_addr}");
    println!("  point scripts/mock_udp_sender.py at this address\n");

    let cfg = UdpConfig::new(bind_addr)
        .with_name("udp_demo")
        .with_sample_rate(100.0)
        .with_queue_capacity(4096);

    let mut sensor: UdpSensor<Packet> = UdpSensor::spawn(cfg)
        .await
        .expect("failed to bind UDP socket");
    let net_metrics = sensor.metrics();
    println!("  listening on {}\n", sensor.local_addr());

    let pipeline_config = PipelineConfig::default()
        .channel_capacity(1024)
        .shutdown_timeout(Duration::from_secs(2));

    let pipeline = MultiStagePipelineBuilder::<Packet, FusedReading, _, _, _, _>::new()
        .config(pipeline_config)
        .ingestion(Map::new(|pkt: Packet| pkt))
        .filtering(Filter::new(|pkt: &Packet| {
            pkt.sensor == "mpu6050" && pkt.accel.is_some() && pkt.gyro.is_some()
        }))
        .aggregation(Map::new(|pkt: Packet| {
            let accel = pkt.accel.unwrap_or([0.0; 3]);
            let gyro = pkt.gyro.unwrap_or([0.0; 3]);
            let mag = (accel[0] * accel[0] + accel[1] * accel[1] + accel[2] * accel[2]).sqrt();
            ImuSample {
                ts_us: pkt.ts_us,
                gyro,
                accel_magnitude: mag,
            }
        }))
        .output(Map::new(|s: ImuSample| {
            let gyro_rate =
                (s.gyro[0] * s.gyro[0] + s.gyro[1] * s.gyro[1] + s.gyro[2] * s.gyro[2]).sqrt();
            FusedReading {
                ts_us: s.ts_us,
                accel_magnitude: s.accel_magnitude,
                gyro_rate,
            }
        }))
        .build();

    let shutdown = Arc::new(AtomicBool::new(false));
    let sigint = Arc::clone(&shutdown);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        sigint.store(true, Ordering::SeqCst);
    });

    let input_tx = pipeline
        .input()
        .cloned()
        .expect("pipeline must have an input");
    let sensor_shutdown = Arc::clone(&shutdown);
    let ingest = tokio::spawn(async move {
        while !sensor_shutdown.load(Ordering::SeqCst) {
            match sensor.recv().await {
                Some(stamped) => {
                    if input_tx.try_send(stamped.data).is_err() {
                        // Pipeline backpressure — drop rather than block.
                    }
                }
                None => break,
            }
        }
    });

    let (stats_shutdown, stats_shutdown_for_spawn) = (Arc::clone(&shutdown), Arc::clone(&shutdown));
    let stats_net = Arc::clone(&net_metrics);
    let stats = thread::spawn(move || {
        let start = Instant::now();
        let mut last_print = Instant::now();
        let mut total_output = 0u64;
        let mut last_packet: Option<FusedReading> = None;

        while !stats_shutdown.load(Ordering::SeqCst) {
            if let Some(reading) = pipeline.try_recv() {
                total_output += 1;
                last_packet = Some(reading);
            } else {
                thread::sleep(Duration::from_millis(1));
            }

            if last_print.elapsed() >= Duration::from_secs(1) {
                let snap = stats_net.snapshot();
                let elapsed = start.elapsed().as_secs_f64();
                let rate = total_output as f64 / elapsed;
                let last_str = last_packet
                    .as_ref()
                    .map(|r| {
                        format!(
                            "ts={:>10}us  |a|={:.2} m/s^2  |g|={:.2} rad/s",
                            r.ts_us, r.accel_magnitude, r.gyro_rate
                        )
                    })
                    .unwrap_or_else(|| "<no data yet>".to_string());
                println!(
                    "t={elapsed:6.1}s  recv={:<8} drop={:<5} parse_err={:<4} out={:<8} rate={:>7.1}/s  {last_str}",
                    snap.packets_received,
                    snap.packets_dropped,
                    snap.parse_errors,
                    total_output,
                    rate,
                );
                last_print = Instant::now();
            }
        }

        (pipeline, total_output)
    });

    // Block until shutdown.
    while !stats_shutdown_for_spawn.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    println!("\nshutdown requested, draining pipeline...");
    let _ = ingest.await;
    let (mut pipeline, total_output) = stats.join().expect("stats thread panicked");
    pipeline.shutdown();
    let _ = pipeline.join();

    let snap = net_metrics.snapshot();
    println!(
        "final: received={} dropped={} parse_err={} socket_err={} out={}",
        snap.packets_received,
        snap.packets_dropped,
        snap.parse_errors,
        snap.socket_errors,
        total_output,
    );
}
