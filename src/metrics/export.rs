//! Metrics export to JSON and CSV formats.
//!
//! This module provides functionality to export pipeline metrics to
//! various formats for external analysis and visualization.

use std::io::{self, Write};

use super::pipeline_metrics::PipelineMetricsSnapshot;
use super::stage_metrics::StageMetricsSnapshot;

/// Export format for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// JSON format.
    Json,
    /// CSV format.
    Csv,
    /// Human-readable text.
    Text,
}

/// Exports metrics to the specified format.
pub struct MetricsExporter;

impl MetricsExporter {
    /// Exports pipeline metrics to JSON.
    pub fn to_json(snapshot: &PipelineMetricsSnapshot) -> String {
        let mut json = String::from("{\n");

        // Summary
        json.push_str(&format!(
            "  \"throughput\": {:.2},\n",
            snapshot.throughput()
        ));
        json.push_str(&format!("  \"total_input\": {},\n", snapshot.total_input));
        json.push_str(&format!("  \"total_output\": {},\n", snapshot.total_output));
        json.push_str(&format!("  \"drop_rate\": {:.4},\n", snapshot.drop_rate()));
        json.push_str(&format!(
            "  \"elapsed_secs\": {:.3},\n",
            snapshot.elapsed_secs
        ));

        // End-to-end latency
        json.push_str("  \"end_to_end_latency\": {\n");
        json.push_str(&format!(
            "    \"count\": {},\n",
            snapshot.end_to_end_latency.count
        ));
        json.push_str(&format!(
            "    \"mean_ns\": {:.2},\n",
            snapshot.end_to_end_latency.mean_ns
        ));
        json.push_str(&format!(
            "    \"p50_ns\": {},\n",
            snapshot.end_to_end_latency.p50_ns
        ));
        json.push_str(&format!(
            "    \"p95_ns\": {},\n",
            snapshot.end_to_end_latency.p95_ns
        ));
        json.push_str(&format!(
            "    \"p99_ns\": {},\n",
            snapshot.end_to_end_latency.p99_ns
        ));
        json.push_str(&format!(
            "    \"p999_ns\": {}\n",
            snapshot.end_to_end_latency.p999_ns
        ));
        json.push_str("  },\n");

        // End-to-end jitter
        json.push_str("  \"end_to_end_jitter\": {\n");
        json.push_str(&format!(
            "    \"count\": {},\n",
            snapshot.end_to_end_jitter.count
        ));
        json.push_str(&format!(
            "    \"mean_ns\": {:.2},\n",
            snapshot.end_to_end_jitter.mean_ns
        ));
        json.push_str(&format!(
            "    \"std_dev_ns\": {:.2}\n",
            snapshot.end_to_end_jitter.std_dev_ns
        ));
        json.push_str("  },\n");

        // Stages
        json.push_str("  \"stages\": [\n");
        for (i, stage) in snapshot.stages.iter().enumerate() {
            json.push_str(&Self::stage_to_json(stage, i == snapshot.stages.len() - 1));
        }
        json.push_str("  ]\n");

        json.push_str("}\n");
        json
    }

    fn stage_to_json(stage: &StageMetricsSnapshot, is_last: bool) -> String {
        let mut json = String::from("    {\n");
        json.push_str(&format!("      \"name\": \"{}\",\n", stage.name));
        json.push_str(&format!("      \"input_count\": {},\n", stage.input_count));
        json.push_str(&format!(
            "      \"output_count\": {},\n",
            stage.output_count
        ));
        json.push_str(&format!(
            "      \"filtered_count\": {},\n",
            stage.filtered_count
        ));
        json.push_str(&format!(
            "      \"dropped_count\": {},\n",
            stage.dropped_count
        ));
        json.push_str(&format!("      \"error_count\": {},\n", stage.error_count));
        json.push_str(&format!(
            "      \"throughput_ratio\": {:.4},\n",
            stage.throughput_ratio()
        ));
        json.push_str("      \"latency\": {\n");
        json.push_str(&format!(
            "        \"mean_ns\": {:.2},\n",
            stage.latency.mean_ns
        ));
        json.push_str(&format!("        \"p99_ns\": {}\n", stage.latency.p99_ns));
        json.push_str("      },\n");
        json.push_str("      \"jitter\": {\n");
        json.push_str(&format!(
            "        \"std_dev_ns\": {:.2}\n",
            stage.jitter.std_dev_ns
        ));
        json.push_str("      }\n");
        json.push_str("    }");
        if !is_last {
            json.push(',');
        }
        json.push('\n');
        json
    }

    /// Exports pipeline metrics to CSV.
    ///
    /// Returns a tuple of (header, rows) for the CSV data.
    pub fn to_csv(snapshot: &PipelineMetricsSnapshot) -> (String, Vec<String>) {
        let header = "timestamp,stage,input_count,output_count,filtered_count,dropped_count,\
                      error_count,throughput_ratio,mean_latency_ns,p99_latency_ns,jitter_ns"
            .to_string();

        let timestamp = snapshot.elapsed_secs;
        let mut rows = Vec::new();

        // Pipeline summary row
        rows.push(format!(
            "{:.3},_pipeline,{},{},{},{},{},{:.4},{:.2},{},{:.2}",
            timestamp,
            snapshot.total_input,
            snapshot.total_output,
            0, // filtered at pipeline level not tracked
            snapshot.total_input.saturating_sub(snapshot.total_output),
            0, // errors at pipeline level not tracked
            if snapshot.total_input > 0 {
                snapshot.total_output as f64 / snapshot.total_input as f64
            } else {
                0.0
            },
            snapshot.end_to_end_latency.mean_ns,
            snapshot.end_to_end_latency.p99_ns,
            snapshot.end_to_end_jitter.std_dev_ns
        ));

        // Per-stage rows
        for stage in &snapshot.stages {
            rows.push(format!(
                "{:.3},{},{},{},{},{},{},{:.4},{:.2},{},{:.2}",
                timestamp,
                stage.name,
                stage.input_count,
                stage.output_count,
                stage.filtered_count,
                stage.dropped_count,
                stage.error_count,
                stage.throughput_ratio(),
                stage.latency.mean_ns,
                stage.latency.p99_ns,
                stage.jitter.std_dev_ns
            ));
        }

        (header, rows)
    }

    /// Exports pipeline metrics to a human-readable text format.
    pub fn to_text(snapshot: &PipelineMetricsSnapshot) -> String {
        snapshot.to_string()
    }

    /// Writes metrics to a writer in the specified format.
    pub fn write<W: Write>(
        writer: &mut W,
        snapshot: &PipelineMetricsSnapshot,
        format: ExportFormat,
    ) -> io::Result<()> {
        match format {
            ExportFormat::Json => {
                writer.write_all(Self::to_json(snapshot).as_bytes())?;
            }
            ExportFormat::Csv => {
                let (header, rows) = Self::to_csv(snapshot);
                writeln!(writer, "{}", header)?;
                for row in rows {
                    writeln!(writer, "{}", row)?;
                }
            }
            ExportFormat::Text => {
                writer.write_all(Self::to_text(snapshot).as_bytes())?;
            }
        }
        Ok(())
    }
}

/// A metrics recorder that periodically saves snapshots.
pub struct MetricsRecorder {
    snapshots: Vec<PipelineMetricsSnapshot>,
    max_snapshots: usize,
}

impl MetricsRecorder {
    /// Creates a new metrics recorder.
    #[must_use]
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: Vec::with_capacity(max_snapshots),
            max_snapshots,
        }
    }

    /// Records a snapshot.
    pub fn record(&mut self, snapshot: PipelineMetricsSnapshot) {
        if self.snapshots.len() >= self.max_snapshots {
            self.snapshots.remove(0);
        }
        self.snapshots.push(snapshot);
    }

    /// Returns all recorded snapshots.
    #[must_use]
    pub fn snapshots(&self) -> &[PipelineMetricsSnapshot] {
        &self.snapshots
    }

    /// Returns the number of recorded snapshots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// Returns whether there are no recorded snapshots.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Exports all snapshots to CSV.
    pub fn export_csv<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        if self.snapshots.is_empty() {
            return Ok(());
        }

        let (header, _) = MetricsExporter::to_csv(&self.snapshots[0]);
        writeln!(writer, "{}", header)?;

        for snapshot in &self.snapshots {
            let (_, rows) = MetricsExporter::to_csv(snapshot);
            for row in rows {
                writeln!(writer, "{}", row)?;
            }
        }

        Ok(())
    }

    /// Exports all snapshots to a JSON array.
    pub fn export_json<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writeln!(writer, "[")?;
        for (i, snapshot) in self.snapshots.iter().enumerate() {
            let json = MetricsExporter::to_json(snapshot);
            // Indent each line
            for line in json.lines() {
                writeln!(writer, "  {}", line)?;
            }
            if i < self.snapshots.len() - 1 {
                writeln!(writer, ",")?;
            }
        }
        writeln!(writer, "]")?;
        Ok(())
    }

    /// Clears all recorded snapshots.
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }
}

impl Default for MetricsRecorder {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Summary statistics for a series of snapshots.
#[derive(Debug, Clone)]
pub struct SummaryStatistics {
    /// Number of snapshots.
    pub count: usize,
    /// Average throughput.
    pub avg_throughput: f64,
    /// Minimum throughput.
    pub min_throughput: f64,
    /// Maximum throughput.
    pub max_throughput: f64,
    /// Average p99 latency (microseconds).
    pub avg_p99_latency_us: f64,
    /// Maximum p99 latency (microseconds).
    pub max_p99_latency_us: f64,
    /// Average jitter (milliseconds).
    pub avg_jitter_ms: f64,
    /// Maximum jitter (milliseconds).
    pub max_jitter_ms: f64,
    /// Overall drop rate.
    pub overall_drop_rate: f64,
}

impl SummaryStatistics {
    /// Computes summary statistics from a slice of snapshots.
    #[must_use]
    pub fn from_snapshots(snapshots: &[PipelineMetricsSnapshot]) -> Self {
        if snapshots.is_empty() {
            return Self {
                count: 0,
                avg_throughput: 0.0,
                min_throughput: 0.0,
                max_throughput: 0.0,
                avg_p99_latency_us: 0.0,
                max_p99_latency_us: 0.0,
                avg_jitter_ms: 0.0,
                max_jitter_ms: 0.0,
                overall_drop_rate: 0.0,
            };
        }

        let count = snapshots.len();

        let throughputs: Vec<f64> = snapshots.iter().map(|s| s.throughput()).collect();
        let latencies: Vec<f64> = snapshots.iter().map(|s| s.p99_latency_us()).collect();
        let jitters: Vec<f64> = snapshots.iter().map(|s| s.jitter_ms()).collect();

        let total_input: u64 = snapshots.iter().map(|s| s.total_input).sum();
        let total_output: u64 = snapshots.iter().map(|s| s.total_output).sum();

        Self {
            count,
            avg_throughput: throughputs.iter().sum::<f64>() / count as f64,
            min_throughput: throughputs.iter().copied().fold(f64::INFINITY, f64::min),
            max_throughput: throughputs.iter().copied().fold(0.0, f64::max),
            avg_p99_latency_us: latencies.iter().sum::<f64>() / count as f64,
            max_p99_latency_us: latencies.iter().copied().fold(0.0, f64::max),
            avg_jitter_ms: jitters.iter().sum::<f64>() / count as f64,
            max_jitter_ms: jitters.iter().copied().fold(0.0, f64::max),
            overall_drop_rate: if total_input > 0 {
                1.0 - (total_output as f64 / total_input as f64)
            } else {
                0.0
            },
        }
    }
}

impl std::fmt::Display for SummaryStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Summary Statistics ({} snapshots):", self.count)?;
        writeln!(
            f,
            "  Throughput: avg={:.1}, min={:.1}, max={:.1} items/sec",
            self.avg_throughput, self.min_throughput, self.max_throughput
        )?;
        writeln!(
            f,
            "  P99 Latency: avg={:.1}us, max={:.1}us",
            self.avg_p99_latency_us, self.max_p99_latency_us
        )?;
        writeln!(
            f,
            "  Jitter: avg={:.2}ms, max={:.2}ms",
            self.avg_jitter_ms, self.max_jitter_ms
        )?;
        writeln!(
            f,
            "  Overall Drop Rate: {:.2}%",
            self.overall_drop_rate * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::jitter::JitterSnapshot;
    use crate::metrics::latency::LatencySnapshot;

    fn make_snapshot() -> PipelineMetricsSnapshot {
        PipelineMetricsSnapshot {
            stages: vec![StageMetricsSnapshot {
                name: "stage1".to_string(),
                input_count: 100,
                output_count: 90,
                filtered_count: 5,
                dropped_count: 5,
                error_count: 0,
                latency: LatencySnapshot {
                    count: 100,
                    mean_ns: 1000.0,
                    min_ns: Some(500),
                    max_ns: Some(2000),
                    p50_ns: 1000,
                    p95_ns: 1500,
                    p99_ns: 1800,
                    p999_ns: 1950,
                },
                jitter: JitterSnapshot {
                    count: 100,
                    mean_ns: 1000.0,
                    std_dev_ns: 200.0,
                    min_ns: Some(500),
                    max_ns: Some(2000),
                },
                bytes_processed: 1000,
                elapsed_secs: 1.0,
            }],
            end_to_end_latency: LatencySnapshot {
                count: 100,
                mean_ns: 5000.0,
                min_ns: Some(2000),
                max_ns: Some(10000),
                p50_ns: 5000,
                p95_ns: 8000,
                p99_ns: 9000,
                p999_ns: 9500,
            },
            end_to_end_jitter: JitterSnapshot {
                count: 100,
                mean_ns: 5000.0,
                std_dev_ns: 1000.0,
                min_ns: Some(2000),
                max_ns: Some(10000),
            },
            total_input: 100,
            total_output: 90,
            elapsed_secs: 1.0,
        }
    }

    #[test]
    fn test_export_json() {
        let snapshot = make_snapshot();
        let json = MetricsExporter::to_json(&snapshot);

        assert!(json.contains("\"throughput\""));
        assert!(json.contains("\"total_input\": 100"));
        assert!(json.contains("\"stages\""));
        assert!(json.contains("\"stage1\""));
    }

    #[test]
    fn test_export_csv() {
        let snapshot = make_snapshot();
        let (header, rows) = MetricsExporter::to_csv(&snapshot);

        assert!(header.contains("timestamp"));
        assert!(header.contains("stage"));
        assert!(header.contains("input_count"));
        assert_eq!(rows.len(), 2); // pipeline + 1 stage
    }

    #[test]
    fn test_export_text() {
        let snapshot = make_snapshot();
        let text = MetricsExporter::to_text(&snapshot);

        assert!(text.contains("Pipeline Metrics"));
        assert!(text.contains("Throughput"));
    }

    #[test]
    fn test_metrics_recorder() {
        let mut recorder = MetricsRecorder::new(3);
        assert!(recorder.is_empty());

        for _ in 0..5 {
            recorder.record(make_snapshot());
        }

        assert_eq!(recorder.len(), 3); // Max snapshots
        assert!(!recorder.is_empty());
    }

    #[test]
    fn test_summary_statistics() {
        let snapshots = vec![make_snapshot(), make_snapshot()];
        let summary = SummaryStatistics::from_snapshots(&snapshots);

        assert_eq!(summary.count, 2);
        assert!(summary.avg_throughput > 0.0);
    }

    #[test]
    fn test_summary_statistics_empty() {
        let summary = SummaryStatistics::from_snapshots(&[]);
        assert_eq!(summary.count, 0);
        assert_eq!(summary.avg_throughput, 0.0);
    }

    #[test]
    fn test_write_to_buffer() {
        let snapshot = make_snapshot();
        let mut buffer = Vec::new();

        MetricsExporter::write(&mut buffer, &snapshot, ExportFormat::Json).unwrap();
        assert!(!buffer.is_empty());

        let json = String::from_utf8(buffer).unwrap();
        assert!(json.contains("throughput"));
    }
}
