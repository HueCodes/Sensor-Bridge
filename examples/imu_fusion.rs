//! Pitch / roll fusion from a simulated MPU-6050.
//!
//! Runs two [`ComplementaryFilter`]s — one for pitch, one for roll —
//! against a synthetic IMU stream that fakes gravity plus a low-frequency
//! sinusoidal tilt plus per-axis gaussian noise on both accel and gyro.
//! The output shows how the complementary filter rejects accelerometer
//! noise while tracking the true tilt without the drift you would see
//! from integrating the gyro alone.
//!
//! No hardware and no network — runs entirely in-process so it is safe to
//! wire into CI via `cargo test --examples` if needed.
//!
//! Run with: `cargo run --example imu_fusion`

use std::time::Duration;

use sensor_bridge::stage::{ComplementaryFilter, ComplementaryInput, Stage};

const SAMPLE_HZ: f32 = 200.0;
const DURATION_S: f32 = 4.0;
const GRAVITY_M_S2: f32 = 9.81;

struct Sim {
    t: f32,
    dt: f32,
    rng_state: u64,
}

impl Sim {
    fn new() -> Self {
        Self {
            t: 0.0,
            dt: 1.0 / SAMPLE_HZ,
            rng_state: 0x5EED_BA5E_u64,
        }
    }

    /// Deterministic xorshift; unit gaussian approximation via CLT over 6 draws.
    fn noise(&mut self, sigma: f32) -> f32 {
        let mut sum = 0.0f32;
        for _ in 0..6 {
            self.rng_state ^= self.rng_state << 13;
            self.rng_state ^= self.rng_state >> 7;
            self.rng_state ^= self.rng_state << 17;
            let u = (self.rng_state as u32) as f32 / u32::MAX as f32;
            sum += u;
        }
        // sum ~ N(3, 0.5) → shift to N(0,1), scale to sigma
        (sum - 3.0) * std::f32::consts::SQRT_2 * sigma
    }

    fn step(&mut self) -> Sample {
        // Truth: pitch oscillates ±0.3 rad at 0.5 Hz, roll at 0.25 Hz.
        let true_pitch = 0.3 * (2.0 * std::f32::consts::PI * 0.5 * self.t).sin();
        let true_roll = 0.2 * (2.0 * std::f32::consts::PI * 0.25 * self.t).sin();

        // Body-frame accel from tilt. Small-angle approximation keeps the
        // example focused on the filter rather than kinematics.
        let accel = [
            -GRAVITY_M_S2 * true_pitch.sin() + self.noise(0.3),
            GRAVITY_M_S2 * true_roll.sin() + self.noise(0.3),
            GRAVITY_M_S2 * true_pitch.cos() * true_roll.cos() + self.noise(0.3),
        ];
        // Finite-difference true rate + gyro bias + noise.
        let next_pitch = 0.3 * (2.0 * std::f32::consts::PI * 0.5 * (self.t + self.dt)).sin();
        let next_roll = 0.2 * (2.0 * std::f32::consts::PI * 0.25 * (self.t + self.dt)).sin();
        let gyro_pitch = (next_pitch - true_pitch) / self.dt + 0.01 + self.noise(0.05);
        let gyro_roll = (next_roll - true_roll) / self.dt + 0.01 + self.noise(0.05);

        self.t += self.dt;

        Sample {
            t: self.t,
            accel,
            gyro_pitch,
            gyro_roll,
            true_pitch,
            true_roll,
        }
    }
}

struct Sample {
    t: f32,
    accel: [f32; 3],
    gyro_pitch: f32,
    gyro_roll: f32,
    true_pitch: f32,
    true_roll: f32,
}

fn main() {
    let mut pitch_filter = ComplementaryFilter::new(0.98);
    let mut roll_filter = ComplementaryFilter::new(0.98);
    let mut sim = Sim::new();

    println!("imu_fusion — {SAMPLE_HZ:.0}Hz, {DURATION_S:.1}s");
    println!(
        "{:>6}  {:>9}  {:>9}  {:>9}  {:>9}  {:>9}  {:>9}",
        "t_ms", "accel_p", "fused_p", "true_p", "accel_r", "fused_r", "true_r"
    );

    let mut err_pitch_sq = 0.0f64;
    let mut err_roll_sq = 0.0f64;
    let mut n = 0u64;

    let total_samples = (SAMPLE_HZ * DURATION_S) as u64;
    for i in 0..total_samples {
        let s = sim.step();
        // Tilt from accel. atan2(-ax, az) is the small-angle pitch;
        // atan2(ay, sqrt(ax^2 + az^2)) is roll. Keep it explicit so the
        // example is readable.
        let accel_pitch = (-s.accel[0]).atan2(s.accel[2]);
        let accel_roll = s.accel[1].atan2((s.accel[0].powi(2) + s.accel[2].powi(2)).sqrt());
        let dt = 1.0 / SAMPLE_HZ;

        let fused_pitch = pitch_filter
            .process(ComplementaryInput {
                accel_angle_rad: accel_pitch,
                gyro_rate_rad_s: s.gyro_pitch,
                dt_s: dt,
            })
            .unwrap();
        let fused_roll = roll_filter
            .process(ComplementaryInput {
                accel_angle_rad: accel_roll,
                gyro_rate_rad_s: s.gyro_roll,
                dt_s: dt,
            })
            .unwrap();

        err_pitch_sq += ((fused_pitch - s.true_pitch) as f64).powi(2);
        err_roll_sq += ((fused_roll - s.true_roll) as f64).powi(2);
        n += 1;

        // Print one sample every 50 (4 Hz).
        if i % 50 == 0 {
            println!(
                "{:>6.0}  {:>9.3}  {:>9.3}  {:>9.3}  {:>9.3}  {:>9.3}  {:>9.3}",
                s.t * 1000.0,
                accel_pitch,
                fused_pitch,
                s.true_pitch,
                accel_roll,
                fused_roll,
                s.true_roll,
            );
        }
    }

    let rmse_pitch = (err_pitch_sq / n as f64).sqrt();
    let rmse_roll = (err_roll_sq / n as f64).sqrt();
    println!(
        "\nRMSE  pitch={:.4} rad  roll={:.4} rad  ({} samples)",
        rmse_pitch, rmse_roll, n
    );

    // Sanity: fused should be better than accel-only. Not asserted —
    // this is an example, not a test — but printing is helpful context.
    std::thread::sleep(Duration::from_millis(10));
}
