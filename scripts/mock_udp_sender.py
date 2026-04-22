#!/usr/bin/env python3
"""Mock UDP sensor sender for the sensor-bridge UDP demo.

Simulates three sensors:
  - "mpu6050": 6-axis IMU (accel xyz in m/s^2, gyro xyz in rad/s)
  - "dht11":   temperature in C + relative humidity
  - "hc-sr04": ultrasonic distance in cm

Sends line-delimited JSON datagrams to a UDP target. Rates, noise, and
duration are configurable.

Usage:
    python3 scripts/mock_udp_sender.py --target 127.0.0.1:9000
    python3 scripts/mock_udp_sender.py --target 127.0.0.1:9000 --imu-hz 200 --duration 30

Requires only the Python standard library.
"""

import argparse
import json
import math
import random
import socket
import sys
import time


def parse_target(value: str) -> tuple[str, int]:
    host, _, port = value.rpartition(":")
    if not host or not port:
        raise argparse.ArgumentTypeError(f"expected host:port, got {value!r}")
    return host, int(port)


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--target", type=parse_target, default=("127.0.0.1", 9000),
                    help="host:port to send UDP packets to (default 127.0.0.1:9000)")
    ap.add_argument("--imu-hz", type=float, default=100.0,
                    help="MPU-6050 sample rate in Hz (default 100)")
    ap.add_argument("--dht-period-s", type=float, default=2.0,
                    help="DHT11 period in seconds (default 2)")
    ap.add_argument("--ultrasonic-hz", type=float, default=10.0,
                    help="HC-SR04 sample rate in Hz (default 10)")
    ap.add_argument("--noise-sigma", type=float, default=0.05,
                    help="Gaussian noise standard deviation on IMU channels")
    ap.add_argument("--duration", type=float, default=0.0,
                    help="Stop after this many seconds (0 = run forever)")
    ap.add_argument("--seed", type=int, default=None,
                    help="Seed the RNG for reproducible streams")
    return ap.parse_args()


def make_imu(t: float, sigma: float) -> dict:
    """Simulate MPU-6050: small oscillation + gaussian noise."""
    w = 2 * math.pi * 0.5  # 0.5 Hz base motion
    return {
        "sensor": "mpu6050",
        "ts_us": int(t * 1_000_000),
        "accel": [
            0.02 * math.sin(w * t) + random.gauss(0.0, sigma),
            0.02 * math.cos(w * t) + random.gauss(0.0, sigma),
            9.81 + random.gauss(0.0, sigma),
        ],
        "gyro": [
            0.01 * math.sin(w * t + 0.5) + random.gauss(0.0, sigma * 0.5),
            0.01 * math.cos(w * t + 0.5) + random.gauss(0.0, sigma * 0.5),
            random.gauss(0.0, sigma * 0.5),
        ],
    }


def make_dht11(t: float) -> dict:
    return {
        "sensor": "dht11",
        "ts_us": int(t * 1_000_000),
        "temp_c": 21.5 + 0.3 * math.sin(t / 30.0) + random.gauss(0.0, 0.1),
        "humidity": 45.0 + 2.0 * math.sin(t / 45.0) + random.gauss(0.0, 0.3),
    }


def make_ultrasonic(t: float) -> dict:
    return {
        "sensor": "hc-sr04",
        "ts_us": int(t * 1_000_000),
        "distance_cm": 50.0 + 10.0 * math.sin(t / 2.0) + random.gauss(0.0, 0.5),
    }


def main() -> int:
    args = parse_args()
    if args.seed is not None:
        random.seed(args.seed)

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    host, port = args.target

    imu_period = 1.0 / args.imu_hz if args.imu_hz > 0 else float("inf")
    us_period = 1.0 / args.ultrasonic_hz if args.ultrasonic_hz > 0 else float("inf")
    dht_period = args.dht_period_s

    start = time.monotonic()
    next_imu = start
    next_dht = start
    next_us = start
    sent = 0

    print(f"sending to udp://{host}:{port}  imu={args.imu_hz}Hz dht=1/{dht_period}s us={args.ultrasonic_hz}Hz")

    try:
        while True:
            now = time.monotonic()
            t = now - start

            if args.duration > 0 and t >= args.duration:
                break

            if now >= next_imu:
                sock.sendto(json.dumps(make_imu(t, args.noise_sigma)).encode(), (host, port))
                next_imu += imu_period
                sent += 1

            if now >= next_dht:
                sock.sendto(json.dumps(make_dht11(t)).encode(), (host, port))
                next_dht += dht_period
                sent += 1

            if now >= next_us:
                sock.sendto(json.dumps(make_ultrasonic(t)).encode(), (host, port))
                next_us += us_period
                sent += 1

            sleep_until = min(next_imu, next_dht, next_us)
            time.sleep(max(0.0, sleep_until - time.monotonic()))

    except KeyboardInterrupt:
        pass
    finally:
        elapsed = time.monotonic() - start
        rate = sent / elapsed if elapsed > 0 else 0.0
        print(f"\nsent {sent} packets in {elapsed:.1f}s ({rate:.1f}/s)")

    return 0


if __name__ == "__main__":
    sys.exit(main())
