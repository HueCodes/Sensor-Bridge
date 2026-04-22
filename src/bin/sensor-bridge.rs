//! `sensor-bridge` CLI.
//!
//! Five subcommands: demo, listen, record, replay, bench. The demo tries
//! to spawn `scripts/mock_udp_sender.py` as a child process; if python is
//! not on the PATH it falls back to an in-process Rust mock so the
//! reviewer sees live data whether Python is installed or not.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use sensor_bridge::drivers::{UdpConfig, UdpSensor};
use sensor_bridge::pipeline::{MultiStagePipelineBuilder, PipelineConfig};
use sensor_bridge::sinks::{CsvRow, CsvSink, Sink};
use sensor_bridge::stage::{Filter, Map};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    name = "sensor-bridge",
    version,
    about = "Real-time sensor pipeline CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Runs a live end-to-end demo. Spawns the Python mock sender when
    /// available, falls back to an in-process mock, and prints running
    /// stats.
    Demo {
        /// UDP bind address.
        #[arg(long, default_value = "127.0.0.1:9000")]
        bind: String,
        /// Demo duration in seconds.
        #[arg(long, default_value_t = 10.0)]
        duration: f32,
    },
    /// Listens on a UDP socket, runs packets through the pipeline, and
    /// prints stats until interrupted.
    Listen {
        /// UDP bind address.
        #[arg(long, default_value = "0.0.0.0:9000")]
        bind: String,
    },
    /// Records raw UDP packets to a CSV file.
    Record {
        /// UDP bind address.
        #[arg(long, default_value = "127.0.0.1:9000")]
        bind: String,
        /// Output CSV path.
        #[arg(long)]
        output: PathBuf,
        /// Stop after this many seconds (0 = run until ctrl-c).
        #[arg(long, default_value_t = 0.0)]
        duration: f32,
    },
    /// Replays a previously recorded CSV file as UDP packets.
    Replay {
        /// Input CSV path (produced by `record`).
        #[arg(long)]
        input: PathBuf,
        /// UDP target to send to.
        #[arg(long, default_value = "127.0.0.1:9001")]
        target: String,
        /// Real-time playback (true) or as-fast-as-possible (false).
        #[arg(long, default_value_t = true)]
        realtime: bool,
    },
    /// Micro-benchmark: measures end-to-end pipeline throughput with
    /// synthetic data and prints a small results table.
    Bench {
        /// Number of items to push.
        #[arg(long, default_value_t = 1_000_000)]
        items: u64,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct Packet {
    sensor: String,
    #[serde(default)]
    ts_us: u64,
    #[serde(default)]
    accel: Option<[f32; 3]>,
    #[serde(default)]
    gyro: Option<[f32; 3]>,
    #[serde(default)]
    temp_c: Option<f32>,
    #[serde(default)]
    distance_cm: Option<f32>,
}

struct Row {
    sensor: String,
    ts_us: u64,
    ax: f32,
    ay: f32,
    az: f32,
    gx: f32,
    gy: f32,
    gz: f32,
    temp_c: f32,
    distance_cm: f32,
}

impl CsvRow for Row {
    fn header() -> &'static [&'static str] {
        &[
            "sensor",
            "ts_us",
            "ax",
            "ay",
            "az",
            "gx",
            "gy",
            "gz",
            "temp_c",
            "distance_cm",
        ]
    }
    fn row(&self) -> Vec<String> {
        vec![
            self.sensor.clone(),
            self.ts_us.to_string(),
            format!("{:.5}", self.ax),
            format!("{:.5}", self.ay),
            format!("{:.5}", self.az),
            format!("{:.5}", self.gx),
            format!("{:.5}", self.gy),
            format!("{:.5}", self.gz),
            format!("{:.3}", self.temp_c),
            format!("{:.3}", self.distance_cm),
        ]
    }
}

impl From<&Packet> for Row {
    fn from(p: &Packet) -> Self {
        let a = p.accel.unwrap_or([0.0; 3]);
        let g = p.gyro.unwrap_or([0.0; 3]);
        Self {
            sensor: p.sensor.clone(),
            ts_us: p.ts_us,
            ax: a[0],
            ay: a[1],
            az: a[2],
            gx: g[0],
            gy: g[1],
            gz: g[2],
            temp_c: p.temp_c.unwrap_or(f32::NAN),
            distance_cm: p.distance_cm.unwrap_or(f32::NAN),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Demo { bind, duration } => cmd_demo(bind.parse()?, duration),
        Cmd::Listen { bind } => cmd_listen(bind.parse()?),
        Cmd::Record {
            bind,
            output,
            duration,
        } => cmd_record(bind.parse()?, output, duration),
        Cmd::Replay {
            input,
            target,
            realtime,
        } => cmd_replay(input, target.parse()?, realtime),
        Cmd::Bench { items } => cmd_bench(items),
    }
}

fn cmd_demo(bind: SocketAddr, duration: f32) -> Result<(), Box<dyn std::error::Error>> {
    println!("demo: binding to {bind}");
    let mut sender = spawn_sender(bind)?;
    run_pipeline_for(bind, Duration::from_secs_f32(duration.max(1.0)))?;
    match &mut sender {
        SenderHandle::Python(child) => {
            let _ = child.kill();
            let _ = child.wait();
        }
        SenderHandle::Rust(flag) => {
            flag.store(true, Ordering::SeqCst);
        }
    }
    Ok(())
}

fn cmd_listen(bind: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    println!("listening on udp://{bind} (ctrl-c to exit)");
    run_pipeline_forever(bind)
}

fn cmd_record(
    bind: SocketAddr,
    output: PathBuf,
    duration: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind(bind)?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut sink = CsvSink::<Row>::create(&output)?;
    println!("recording udp://{bind} → {}", output.display());

    let deadline = if duration > 0.0 {
        Some(Instant::now() + Duration::from_secs_f32(duration))
    } else {
        None
    };

    let mut buf = [0u8; 2048];
    let shutdown = install_sigint()?;
    let mut count = 0u64;
    while !shutdown.load(Ordering::SeqCst) {
        if let Some(d) = deadline {
            if Instant::now() >= d {
                break;
            }
        }
        match socket.recv(&mut buf) {
            Ok(n) => {
                if let Ok(pkt) = serde_json::from_slice::<Packet>(&buf[..n]) {
                    let row: Row = (&pkt).into();
                    sink.write(row)?;
                    count += 1;
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(e.into()),
        }
    }
    sink.close()?;
    println!("\nwrote {} rows", count);
    Ok(())
}

fn cmd_replay(
    input: PathBuf,
    target: SocketAddr,
    realtime: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::open(&input)?;
    let reader = BufReader::new(file);
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    println!("replaying {} → udp://{target}", input.display());

    let mut first_ts: Option<u64> = None;
    let start = Instant::now();
    let mut lines = reader.lines();
    // Skip the header.
    let _ = lines.next();

    for line in lines {
        let line = line?;
        let mut parts = line.split(',');
        let sensor = parts.next().unwrap_or("").to_string();
        let ts_us: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let ax: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let ay: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let az: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let gx: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let gy: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let gz: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let temp_c: f32 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(f32::NAN);
        let distance_cm: f32 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(f32::NAN);

        if realtime {
            let first = *first_ts.get_or_insert(ts_us);
            let target_elapsed = Duration::from_micros(ts_us.saturating_sub(first));
            let actual_elapsed = start.elapsed();
            if target_elapsed > actual_elapsed {
                thread::sleep(target_elapsed - actual_elapsed);
            }
        }

        let mut map = serde_json::Map::new();
        map.insert(
            "sensor".to_string(),
            serde_json::Value::String(sensor.clone()),
        );
        map.insert("ts_us".to_string(), serde_json::Value::Number(ts_us.into()));
        if sensor == "mpu6050" {
            map.insert(
                "accel".to_string(),
                serde_json::json!([ax as f64, ay as f64, az as f64]),
            );
            map.insert(
                "gyro".to_string(),
                serde_json::json!([gx as f64, gy as f64, gz as f64]),
            );
        }
        if temp_c.is_finite() {
            map.insert("temp_c".to_string(), serde_json::json!(temp_c as f64));
        }
        if distance_cm.is_finite() {
            map.insert(
                "distance_cm".to_string(),
                serde_json::json!(distance_cm as f64),
            );
        }

        let bytes = serde_json::to_vec(&serde_json::Value::Object(map))?;
        socket.send_to(&bytes, target)?;
    }
    Ok(())
}

fn cmd_bench(items: u64) -> Result<(), Box<dyn std::error::Error>> {
    use sensor_bridge::stage::Identity;
    let pipeline_config = PipelineConfig::default().channel_capacity(4096);
    let mut pipeline = MultiStagePipelineBuilder::<u64, u64, _, _, _, _>::new()
        .config(pipeline_config)
        .ingestion(Map::new(|x: u64| x))
        .filtering(Filter::new(|_x: &u64| true))
        .aggregation(Map::new(|x: u64| x.wrapping_add(1)))
        .output(Identity::new())
        .build();
    let tx = pipeline.input().cloned().unwrap();

    let t0 = Instant::now();
    let producer = thread::spawn(move || {
        for i in 0..items {
            while tx.try_send(i).is_err() {
                thread::yield_now();
            }
        }
    });

    let mut received = 0u64;
    while received < items {
        if pipeline.try_recv().is_some() {
            received += 1;
        }
    }
    producer.join().unwrap();
    let elapsed = t0.elapsed();

    pipeline.shutdown();
    let _ = pipeline.join();

    let rate = items as f64 / elapsed.as_secs_f64();
    println!(
        "bench: {items} items in {:.3}s  =>  {:.0} items/s",
        elapsed.as_secs_f64(),
        rate
    );
    Ok(())
}

enum SenderHandle {
    Python(Child),
    Rust(Arc<AtomicBool>),
}

fn spawn_sender(bind: SocketAddr) -> Result<SenderHandle, Box<dyn std::error::Error>> {
    let target = format!("{bind}");
    let script = find_script();
    if let Some(path) = script {
        if which("python3").is_some() {
            println!("spawning python mock sender ({})", path.display());
            let child = Command::new("python3")
                .arg(&path)
                .arg("--target")
                .arg(&target)
                .arg("--duration")
                .arg("0")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            return Ok(SenderHandle::Python(child));
        }
    }

    println!("python3 not available — using in-process Rust mock sender");
    let flag = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&flag);
    thread::spawn(move || {
        let sock = UdpSocket::bind("0.0.0.0:0").expect("bind mock");
        let start = Instant::now();
        let mut seq = 0u64;
        while !stop.load(Ordering::SeqCst) {
            let t = start.elapsed().as_secs_f32();
            let ax = 0.02 * t.sin();
            let ay = 0.02 * t.cos();
            let az = 9.81;
            let gx = 0.01 * t.sin();
            let gy = 0.01 * t.cos();
            let body = format!(
                r#"{{"sensor":"mpu6050","ts_us":{},"accel":[{ax:.4},{ay:.4},{az:.4}],"gyro":[{gx:.4},{gy:.4},0.0]}}"#,
                (start.elapsed().as_micros() as u64)
            );
            let _ = sock.send_to(body.as_bytes(), target.as_str());
            seq += 1;
            thread::sleep(Duration::from_millis(10));
            if seq % 100 == 0 && stop.load(Ordering::SeqCst) {
                break;
            }
        }
    });
    Ok(SenderHandle::Rust(flag))
}

fn find_script() -> Option<PathBuf> {
    let candidates = [
        "scripts/mock_udp_sender.py",
        "../scripts/mock_udp_sender.py",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn install_sigint() -> io::Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    let clone = Arc::clone(&flag);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    thread::spawn(move || {
        rt.block_on(async move {
            let _ = tokio::signal::ctrl_c().await;
            clone.store(true, Ordering::SeqCst);
        });
    });
    Ok(flag)
}

fn run_pipeline_for(
    bind: SocketAddr,
    duration: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let cfg = UdpConfig::new(bind).with_queue_capacity(4096);
        let mut sensor: UdpSensor<Packet> = UdpSensor::spawn(cfg).await?;
        let net_metrics = sensor.metrics();

        let pipeline_config = PipelineConfig::default().channel_capacity(1024);
        let mut pipeline = MultiStagePipelineBuilder::<Packet, u64, _, _, _, _>::new()
            .config(pipeline_config)
            .ingestion(Map::new(|p: Packet| p))
            .filtering(Filter::new(|_: &Packet| true))
            .aggregation(Map::new(|p: Packet| p))
            .output(Map::new(|p: Packet| p.ts_us))
            .build();
        let tx = pipeline.input().cloned().unwrap();

        let ingest = tokio::spawn(async move {
            while let Some(stamped) = sensor.recv().await {
                if tx.try_send(stamped.data).is_err() {
                    // backpressure
                }
            }
        });

        let start = Instant::now();
        let mut last_out = 0u64;
        let mut total_out = 0u64;
        let mut last_print = Instant::now();
        while start.elapsed() < duration {
            if pipeline.try_recv().is_some() {
                total_out += 1;
            } else {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            if last_print.elapsed() >= Duration::from_secs(1) {
                let snap = net_metrics.snapshot();
                let d = total_out - last_out;
                last_out = total_out;
                println!(
                    "t={:5.1}s  recv={:<6} drop={:<4} parse_err={:<3} out={:<6} (+{d}/s)",
                    start.elapsed().as_secs_f32(),
                    snap.packets_received,
                    snap.packets_dropped,
                    snap.parse_errors,
                    total_out,
                );
                io::stdout().flush().ok();
                last_print = Instant::now();
            }
        }

        ingest.abort();
        pipeline.shutdown();
        let _ = pipeline.join();
        let snap = net_metrics.snapshot();
        println!(
            "final: recv={} drop={} parse_err={} out={}",
            snap.packets_received, snap.packets_dropped, snap.parse_errors, total_out
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}

fn run_pipeline_forever(bind: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let cfg = UdpConfig::new(bind).with_queue_capacity(4096);
        let mut sensor: UdpSensor<Packet> = UdpSensor::spawn(cfg).await?;
        let net_metrics = sensor.metrics();

        let pipeline_config = PipelineConfig::default().channel_capacity(1024);
        let pipeline = MultiStagePipelineBuilder::<Packet, u64, _, _, _, _>::new()
            .config(pipeline_config)
            .ingestion(Map::new(|p: Packet| p))
            .filtering(Filter::new(|_: &Packet| true))
            .aggregation(Map::new(|p: Packet| p))
            .output(Map::new(|p: Packet| p.ts_us))
            .build();
        let tx = pipeline.input().cloned().unwrap();

        let ingest = tokio::spawn(async move {
            while let Some(stamped) = sensor.recv().await {
                if tx.try_send(stamped.data).is_err() {}
            }
        });

        let (_, _) = tokio::join!(
            async move {
                let _ = tokio::signal::ctrl_c().await;
            },
            async move {
                let mut total = 0u64;
                let mut last_print = Instant::now();
                loop {
                    while pipeline.try_recv().is_some() {
                        total += 1;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    if last_print.elapsed() >= Duration::from_secs(1) {
                        let snap = net_metrics.snapshot();
                        println!(
                            "recv={} drop={} parse_err={} out={}",
                            snap.packets_received, snap.packets_dropped, snap.parse_errors, total
                        );
                        last_print = Instant::now();
                    }
                }
            }
        );
        ingest.abort();
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
