//! Multi-sensor fusion example.
//!
//! This example demonstrates:
//! - Running multiple sensors concurrently
//! - Timestamp-based synchronization
//! - Sensor fusion

use sensor_pipeline::{
    buffer::RingBuffer,
    sensor::{MockImu, NoiseConfig, Sensor},
    stage::TimestampSync,
    timestamp::Timestamped,
    ImuReading,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A second "sensor" that measures something different (e.g., barometer for altitude).
#[derive(Debug, Clone, Copy, Default)]
struct BarometerReading {
    pressure_pa: f32,
    temperature_c: f32,
}

fn main() {
    println!("=== Multi-Sensor Fusion Example ===\n");

    // Create buffers for both sensors
    let imu_buffer = Box::leak(Box::new(
        RingBuffer::<Timestamped<ImuReading>, 512>::new(),
    ));
    let baro_buffer = Box::leak(Box::new(
        RingBuffer::<Timestamped<BarometerReading>, 128>::new(),
    ));

    let (imu_producer, imu_consumer) = imu_buffer.split();
    let (baro_producer, baro_consumer) = baro_buffer.split();

    let running = Arc::new(AtomicBool::new(true));

    // IMU producer - 100Hz
    let running_imu = Arc::clone(&running);
    let imu_handle = thread::spawn(move || {
        let mut imu = MockImu::new(100.0)
            .with_name("imu")
            .with_noise(NoiseConfig::low(), NoiseConfig::low());

        while running_imu.load(Ordering::Relaxed) {
            if let Ok(reading) = imu.sample() {
                let _ = imu_producer.push(reading);
            }
            thread::sleep(Duration::from_millis(10));
        }
    });

    // Barometer producer - 20Hz (slower sensor)
    let running_baro = Arc::clone(&running);
    let baro_handle = thread::spawn(move || {
        let clock = sensor_pipeline::timestamp::MonotonicClock::new();
        let mut seq = 0u64;

        while running_baro.load(Ordering::Relaxed) {
            let reading = BarometerReading {
                pressure_pa: 101325.0 + (seq as f32 * 0.1).sin() * 100.0,
                temperature_c: 25.0 + (seq as f32 * 0.05).cos() * 2.0,
            };
            let ts = clock.stamp(reading);

            let _ = baro_producer.push(Timestamped {
                timestamp_ns: ts.timestamp_ns,
                seq,
                data: reading,
            });

            seq += 1;
            thread::sleep(Duration::from_millis(50)); // 20Hz
        }
    });

    // Fusion consumer
    let running_fusion = Arc::clone(&running);
    let fusion_handle = thread::spawn(move || {
        // Synchronizer with 50ms tolerance (to accommodate the slower barometer)
        let mut sync: TimestampSync<ImuReading, BarometerReading, 64> =
            TimestampSync::with_tolerance_ms(50);

        println!("Fusion: Starting with 50ms sync tolerance\n");
        println!("Time(ms)  Accel Mag  Pressure(Pa)  Temp(C)  Sync Diff(ms)");
        println!("--------  ---------  ------------  -------  -------------");

        let mut fused_count = 0u64;
        let mut last_print = std::time::Instant::now();

        while running_fusion.load(Ordering::Relaxed) {
            // Feed barometer data into sync buffer
            while let Some(baro) = baro_consumer.pop() {
                sync.push_secondary(baro);
            }

            // Try to match IMU data with barometer
            while let Some(imu) = imu_consumer.pop() {
                if let Some(pair) = sync.match_primary(imu) {
                    fused_count += 1;

                    // Print occasionally
                    if last_print.elapsed() >= Duration::from_millis(200) {
                        let ts_ms = pair.timestamp_ns() / 1_000_000;
                        let accel_mag = pair.primary.data.accel.magnitude();
                        let pressure = pair.secondary.data.pressure_pa;
                        let temp = pair.secondary.data.temperature_c;
                        let diff_ms = pair.time_diff_ns as f64 / 1_000_000.0;

                        println!(
                            "{ts_ms:8}  {accel_mag:9.3}  {pressure:12.1}  {temp:7.2}  {diff_ms:13.2}"
                        );
                        last_print = std::time::Instant::now();
                    }
                }
            }

            thread::yield_now();
        }

        println!("\nFusion: Statistics:");
        println!("  Fused pairs: {fused_count}");
        println!("  Unmatched IMU: {}", sync.unmatched_count);
        println!("  Buffer size: {}", sync.buffer_len());
    });

    // Run for 3 seconds
    println!("Running for 3 seconds...\n");
    thread::sleep(Duration::from_secs(3));

    running.store(false, Ordering::Relaxed);

    imu_handle.join().expect("IMU thread panicked");
    baro_handle.join().expect("Baro thread panicked");
    fusion_handle.join().expect("Fusion thread panicked");

    println!("\n=== Done ===");
}
