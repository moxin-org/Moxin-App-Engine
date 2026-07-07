//! HTTP transport: batches SpanRecords and POSTs to `POST /v1/traces`.

use super::types::{ObservatoryError, SpanRecord};
use reqwest::Client;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Background task that receives spans from the layer and POSTs them to the Observatory.
pub struct ObservatoryTransport {
    endpoint: String,
    rx: mpsc::Receiver<SpanRecord>,
    client: Client,
}

impl ObservatoryTransport {
    pub fn new(endpoint: String, rx: mpsc::Receiver<SpanRecord>) -> Self {
        Self {
            endpoint,
            rx,
            client: Client::new(),
        }
    }

    /// Run the transport loop. Drains the channel every `flush_interval_ms` milliseconds
    /// and POSTs all buffered spans to `{endpoint}/v1/traces`.
    ///
    /// This method consumes `self` and should be spawned as a Tokio task:
    /// ```rust,no_run
    /// tokio::spawn(transport.run(100));
    /// ```
    pub async fn run(mut self, flush_interval_ms: u64) {
        let interval = Duration::from_millis(flush_interval_ms);
        let mut ticker = tokio::time::interval(interval);
        let mut buffer: Vec<SpanRecord> = Vec::with_capacity(64);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        let batch = std::mem::take(&mut buffer);
                        if let Err(e) = self.flush(batch).await {
                            warn!("Observatory flush error: {}", e);
                        }
                    }
                }
                Some(record) = self.rx.recv() => {
                    buffer.push(record);
                    // If buffer is large enough, flush eagerly without waiting
                    if buffer.len() >= 512 {
                        let batch = std::mem::take(&mut buffer);
                        if let Err(e) = self.flush(batch).await {
                            warn!("Observatory flush error (eager): {}", e);
                        }
                    }
                }
                else => {
                    // Channel closed — flush remaining spans and exit
                    if !buffer.is_empty() {
                        let batch = std::mem::take(&mut buffer);
                        let _ = self.flush(batch).await;
                    }
                    break;
                }
            }
        }
    }

    async fn flush(&self, spans: Vec<SpanRecord>) -> Result<(), ObservatoryError> {
        let url = format!("{}/v1/traces", self.endpoint);
        let body = serde_json::to_vec(&spans)
            .map_err(|e| ObservatoryError::Serialization(e.to_string()))?;

        debug!("Observatory: flushing {} spans to {}", spans.len(), url);

        self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| ObservatoryError::Transport(e.to_string()))?;

        Ok(())
    }
}
