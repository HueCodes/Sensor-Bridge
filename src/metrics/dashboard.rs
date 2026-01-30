//! Live console dashboard for metrics visualization.
//!
//! This module provides a real-time console dashboard for monitoring
//! pipeline performance metrics.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::pipeline_metrics::{PerformanceTargets, PipelineMetricsAggregator, PipelineMetricsSnapshot};

/// Configuration for the dashboard.
#[derive(Debug, Clone)]
pub struct DashboardConfig {
    /// Refresh interval.
    pub refresh_interval: Duration,
    /// Whether to show per-stage metrics.
    pub show_stages: bool,
    /// Whether to show target comparison.
    pub show_targets: bool,
    /// Performance targets to compare against.
    pub targets: PerformanceTargets,
    /// Whether to clear screen on each refresh.
    pub clear_screen: bool,
    /// Maximum history length for sparklines.
    pub history_length: usize,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            refresh_interval: Duration::from_millis(500),
            show_stages: true,
            show_targets: true,
            targets: PerformanceTargets::default(),
            clear_screen: true,
            history_length: 60,
        }
    }
}

impl DashboardConfig {
    /// Creates a new dashboard configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the refresh interval.
    #[must_use]
    pub fn refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh_interval = interval;
        self
    }

    /// Sets whether to show per-stage metrics.
    #[must_use]
    pub fn show_stages(mut self, show: bool) -> Self {
        self.show_stages = show;
        self
    }

    /// Sets whether to show target comparison.
    #[must_use]
    pub fn show_targets(mut self, show: bool) -> Self {
        self.show_targets = show;
        self
    }

    /// Sets the performance targets.
    #[must_use]
    pub fn targets(mut self, targets: PerformanceTargets) -> Self {
        self.targets = targets;
        self
    }

    /// Sets whether to clear screen on refresh.
    #[must_use]
    pub fn clear_screen(mut self, clear: bool) -> Self {
        self.clear_screen = clear;
        self
    }
}

/// A live console dashboard for pipeline metrics.
pub struct Dashboard {
    config: DashboardConfig,
    metrics: Arc<PipelineMetricsAggregator>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    throughput_history: Vec<f64>,
    latency_history: Vec<f64>,
}

impl Dashboard {
    /// Creates a new dashboard.
    #[must_use]
    pub fn new(metrics: Arc<PipelineMetricsAggregator>, config: DashboardConfig) -> Self {
        Self {
            config,
            metrics,
            shutdown: Arc::new(AtomicBool::new(false)),
            thread: None,
            throughput_history: Vec::new(),
            latency_history: Vec::new(),
        }
    }

    /// Starts the dashboard in a background thread.
    pub fn start(&mut self) {
        let config = self.config.clone();
        let metrics = Arc::clone(&self.metrics);
        let shutdown = Arc::clone(&self.shutdown);

        let thread = thread::Builder::new()
            .name("dashboard".into())
            .spawn(move || {
                let mut dashboard = DashboardRenderer::new(config);
                while !shutdown.load(Ordering::Relaxed) {
                    let snapshot = metrics.snapshot();
                    dashboard.render(&snapshot);
                    thread::sleep(dashboard.config.refresh_interval);
                }
            })
            .expect("failed to spawn dashboard thread");

        self.thread = Some(thread);
    }

    /// Stops the dashboard.
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Renders the dashboard once (for non-threaded use).
    pub fn render_once(&mut self) {
        let snapshot = self.metrics.snapshot();

        // Update history
        self.throughput_history.push(snapshot.throughput());
        self.latency_history.push(snapshot.p99_latency_us());

        if self.throughput_history.len() > self.config.history_length {
            self.throughput_history.remove(0);
        }
        if self.latency_history.len() > self.config.history_length {
            self.latency_history.remove(0);
        }

        let mut renderer = DashboardRenderer::new(self.config.clone());
        renderer.throughput_history = self.throughput_history.clone();
        renderer.latency_history = self.latency_history.clone();
        renderer.render(&snapshot);
    }
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Internal renderer for the dashboard.
struct DashboardRenderer {
    config: DashboardConfig,
    start_time: Instant,
    throughput_history: Vec<f64>,
    latency_history: Vec<f64>,
}

impl DashboardRenderer {
    fn new(config: DashboardConfig) -> Self {
        Self {
            config,
            start_time: Instant::now(),
            throughput_history: Vec::new(),
            latency_history: Vec::new(),
        }
    }

    fn render(&mut self, snapshot: &PipelineMetricsSnapshot) {
        let mut output = String::new();

        // Clear screen if configured
        if self.config.clear_screen {
            output.push_str("\x1B[2J\x1B[H");
        }

        // Header
        output.push_str(&self.render_header());
        output.push('\n');

        // Summary
        output.push_str(&self.render_summary(snapshot));
        output.push('\n');

        // Performance check
        if self.config.show_targets {
            output.push_str(&self.render_targets(snapshot));
            output.push('\n');
        }

        // Throughput sparkline
        self.throughput_history.push(snapshot.throughput());
        if self.throughput_history.len() > self.config.history_length {
            self.throughput_history.remove(0);
        }
        output.push_str(&format!(
            "Throughput: {}\n",
            Self::sparkline(&self.throughput_history)
        ));

        // Latency sparkline
        self.latency_history.push(snapshot.p99_latency_us());
        if self.latency_history.len() > self.config.history_length {
            self.latency_history.remove(0);
        }
        output.push_str(&format!(
            "P99 Latency: {}\n",
            Self::sparkline(&self.latency_history)
        ));

        output.push('\n');

        // Per-stage metrics
        if self.config.show_stages {
            output.push_str(&self.render_stages(snapshot));
        }

        // Print all at once to avoid flickering
        print!("{}", output);
        let _ = io::stdout().flush();
    }

    fn render_header(&self) -> String {
        let elapsed = self.start_time.elapsed();
        format!(
            "=== Sensor Pipeline Dashboard === [Uptime: {:02}:{:02}:{:02}]",
            elapsed.as_secs() / 3600,
            (elapsed.as_secs() % 3600) / 60,
            elapsed.as_secs() % 60
        )
    }

    fn render_summary(&self, snapshot: &PipelineMetricsSnapshot) -> String {
        format!(
            "Throughput: {:>10.1} items/sec | P99 Latency: {:>8.1} us | Jitter: {:>6.2} ms | Drop: {:>5.2}%",
            snapshot.throughput(),
            snapshot.p99_latency_us(),
            snapshot.jitter_ms(),
            snapshot.drop_rate() * 100.0
        )
    }

    fn render_targets(&self, snapshot: &PipelineMetricsSnapshot) -> String {
        let check = snapshot.check_targets(&self.config.targets);
        let status = if check.all_ok() { "[OK]" } else { "[WARN]" };

        format!(
            "Targets {} Throughput: {} | Latency: {} | Jitter: {}",
            status,
            Self::status_indicator(check.throughput_ok),
            Self::status_indicator(check.latency_ok),
            Self::status_indicator(check.jitter_ok)
        )
    }

    fn render_stages(&self, snapshot: &PipelineMetricsSnapshot) -> String {
        let mut output = String::from("Per-Stage Metrics:\n");
        output.push_str(&format!(
            "{:<12} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}\n",
            "Stage", "In", "Out", "Filtered", "Dropped", "P99 (us)", "Jitter (us)"
        ));
        output.push_str(&"-".repeat(74));
        output.push('\n');

        for stage in &snapshot.stages {
            output.push_str(&format!(
                "{:<12} {:>10} {:>10} {:>10} {:>10} {:>10.1} {:>10.1}\n",
                stage.name,
                stage.input_count,
                stage.output_count,
                stage.filtered_count,
                stage.dropped_count,
                stage.latency.p99_us(),
                stage.jitter.std_dev_us()
            ));
        }

        output
    }

    fn status_indicator(ok: bool) -> &'static str {
        if ok { "[OK]" } else { "[X]" }
    }

    fn sparkline(values: &[f64]) -> String {
        if values.is_empty() {
            return String::new();
        }

        let chars = ['_', '.', '-', '~', '^', '*', '#'];
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let range = max - min;

        if range < f64::EPSILON {
            return chars[3].to_string().repeat(values.len().min(30));
        }

        values
            .iter()
            .take(30)
            .map(|&v| {
                let normalized = (v - min) / range;
                let idx = ((normalized * (chars.len() - 1) as f64) as usize).min(chars.len() - 1);
                chars[idx]
            })
            .collect()
    }
}

/// A simple one-line status display.
pub struct StatusLine {
    metrics: Arc<PipelineMetricsAggregator>,
    targets: PerformanceTargets,
}

impl StatusLine {
    /// Creates a new status line.
    #[must_use]
    pub fn new(metrics: Arc<PipelineMetricsAggregator>) -> Self {
        Self {
            metrics,
            targets: PerformanceTargets::default(),
        }
    }

    /// Sets the performance targets.
    #[must_use]
    pub fn with_targets(mut self, targets: PerformanceTargets) -> Self {
        self.targets = targets;
        self
    }

    /// Renders the status line.
    #[must_use]
    pub fn render(&self) -> String {
        let snapshot = self.metrics.snapshot();
        let check = snapshot.check_targets(&self.targets);
        let status = if check.all_ok() { "OK" } else { "WARN" };

        format!(
            "[{}] {:>8.0} items/sec | p99: {:>6.1}us | jitter: {:>5.2}ms | in: {} out: {}",
            status,
            snapshot.throughput(),
            snapshot.p99_latency_us(),
            snapshot.jitter_ms(),
            snapshot.total_input,
            snapshot.total_output
        )
    }

    /// Prints the status line.
    pub fn print(&self) {
        print!("\r{}", self.render());
        let _ = io::stdout().flush();
    }
}

/// A builder for creating dashboard instances.
pub struct DashboardBuilder {
    config: DashboardConfig,
}

impl DashboardBuilder {
    /// Creates a new dashboard builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: DashboardConfig::default(),
        }
    }

    /// Sets the refresh interval.
    #[must_use]
    pub fn refresh_interval(mut self, interval: Duration) -> Self {
        self.config.refresh_interval = interval;
        self
    }

    /// Enables or disables per-stage metrics.
    #[must_use]
    pub fn show_stages(mut self, show: bool) -> Self {
        self.config.show_stages = show;
        self
    }

    /// Enables or disables target comparison.
    #[must_use]
    pub fn show_targets(mut self, show: bool) -> Self {
        self.config.show_targets = show;
        self
    }

    /// Sets the performance targets.
    #[must_use]
    pub fn targets(mut self, targets: PerformanceTargets) -> Self {
        self.config.targets = targets;
        self
    }

    /// Enables or disables screen clearing.
    #[must_use]
    pub fn clear_screen(mut self, clear: bool) -> Self {
        self.config.clear_screen = clear;
        self
    }

    /// Builds the dashboard.
    #[must_use]
    pub fn build(self, metrics: Arc<PipelineMetricsAggregator>) -> Dashboard {
        Dashboard::new(metrics, self.config)
    }
}

impl Default for DashboardBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_config() {
        let config = DashboardConfig::new()
            .refresh_interval(Duration::from_secs(1))
            .show_stages(false)
            .show_targets(true)
            .clear_screen(false);

        assert_eq!(config.refresh_interval, Duration::from_secs(1));
        assert!(!config.show_stages);
        assert!(config.show_targets);
        assert!(!config.clear_screen);
    }

    #[test]
    fn test_sparkline() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sparkline = DashboardRenderer::sparkline(&values);
        assert!(!sparkline.is_empty());
        assert_eq!(sparkline.len(), 5);
    }

    #[test]
    fn test_sparkline_constant() {
        let values = vec![5.0, 5.0, 5.0, 5.0, 5.0];
        let sparkline = DashboardRenderer::sparkline(&values);
        assert_eq!(sparkline.len(), 5);
    }

    #[test]
    fn test_sparkline_empty() {
        let values: Vec<f64> = vec![];
        let sparkline = DashboardRenderer::sparkline(&values);
        assert!(sparkline.is_empty());
    }

    #[test]
    fn test_status_line() {
        let metrics = Arc::new(PipelineMetricsAggregator::new());
        let status = StatusLine::new(metrics);
        let rendered = status.render();
        assert!(rendered.contains("items/sec"));
    }

    #[test]
    fn test_dashboard_builder() {
        let metrics = Arc::new(PipelineMetricsAggregator::new());
        let dashboard = DashboardBuilder::new()
            .refresh_interval(Duration::from_millis(100))
            .show_stages(true)
            .build(metrics);

        assert!(dashboard.config.show_stages);
    }
}
