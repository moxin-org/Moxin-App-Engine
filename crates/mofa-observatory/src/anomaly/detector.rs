use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// An anomaly event emitted when a metric deviates more than 2σ from baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyEvent {
    pub agent_id: String,
    pub metric: String,
    pub current_value: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub severity: String,
}

/// Tracks rolling latency and error-rate statistics for a single agent.
pub struct AgentStats {
    pub agent_id: String,
    pub latency_window: VecDeque<f64>,
    pub error_window: VecDeque<bool>,
    pub window_size: usize,
    /// If set, anomaly events are POSTed to this URL.
    pub webhook_url: Option<String>,
    client: Client,
}

impl AgentStats {
    pub fn new(agent_id: impl Into<String>, window_size: usize) -> Self {
        Self {
            agent_id: agent_id.into(),
            latency_window: VecDeque::new(),
            error_window: VecDeque::new(),
            window_size,
            webhook_url: None,
            client: Client::new(),
        }
    }

    pub fn with_webhook(mut self, url: impl Into<String>) -> Self {
        self.webhook_url = Some(url.into());
        self
    }

    /// Record a new observation and return an anomaly event if detected.
    pub async fn record(&mut self, latency_ms: f64, is_error: bool) -> Option<AnomalyEvent> {
        self.latency_window.push_back(latency_ms);
        self.error_window.push_back(is_error);
        if self.latency_window.len() > self.window_size {
            self.latency_window.pop_front();
            self.error_window.pop_front();
        }

        let anomaly = self.detect_latency_anomaly(latency_ms);
        if let Some(ref event) = anomaly {
            if let Some(ref url) = self.webhook_url {
                let _ = self
                    .client
                    .post(url)
                    .json(event)
                    .send()
                    .await;
            }
        }
        anomaly
    }

    fn detect_latency_anomaly(&self, current: f64) -> Option<AnomalyEvent> {
        if self.latency_window.len() < 10 {
            return None; // Not enough data for a baseline
        }
        let n = self.latency_window.len() as f64;
        let mean = self.latency_window.iter().sum::<f64>() / n;
        let variance = self
            .latency_window
            .iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>()
            / n;
        let std_dev = variance.sqrt();

        if std_dev > 0.0 && (current - mean).abs() > 2.0 * std_dev {
            Some(AnomalyEvent {
                agent_id: self.agent_id.clone(),
                metric: "latency_ms".to_string(),
                current_value: current,
                mean,
                std_dev,
                severity: if (current - mean).abs() > 3.0 * std_dev {
                    "critical"
                } else {
                    "warning"
                }
                .to_string(),
            })
        } else {
            None
        }
    }

    /// Rolling P99 latency (approximate).
    pub fn p99_latency(&self) -> Option<f64> {
        if self.latency_window.is_empty() {
            return None;
        }
        let mut sorted: Vec<f64> = self.latency_window.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let idx = ((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1);
        Some(sorted[idx])
    }
}
