//! Review Context
//! Context information captured for review

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Execution trace snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Execution steps
    pub steps: Vec<ExecutionStep>,
    /// Total duration (milliseconds)
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub step_id: String,
    pub step_type: String,
    pub timestamp_ms: u64,
    pub input: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Telemetry snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    /// Metrics at time of review request
    pub metrics: HashMap<String, f64>,
    /// Logs (last N lines)
    pub recent_logs: Vec<String>,
    /// Performance data
    pub performance: Option<PerformanceData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceData {
    pub cpu_usage: f64,
    pub memory_usage_bytes: u64,
    pub latency_ms: u64,
}

/// Diff representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    pub diff_type: String, // "text", "json", "structured"
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub change_type: String, // "added", "removed", "modified"
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
}

/// Structured auditing data for high-integrity workflows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditingData {
    /// The core intent of the action
    pub intent: String,
    /// The final result or proposal
    pub result: String,
    /// Steps taken during the reasoning process
    pub relevant_trace_steps: Vec<String>,
    /// Domain-specific financial/technical metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// The status of the internal policy check
    pub policy_status: String,
}

/// Review context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContext {
    /// Execution trace
    pub execution_trace: ExecutionTrace,
    /// Input data
    pub input_data: serde_json::Value,
    /// Output data (if available)
    pub output_data: Option<serde_json::Value>,
    /// Diff (if applicable)
    pub diff: Option<Diff>,
    /// Telemetry snapshot
    pub telemetry: TelemetrySnapshot,
    /// Additional context
    pub additional: HashMap<String, serde_json::Value>,
}

impl ReviewContext {
    pub fn new(execution_trace: ExecutionTrace, input_data: serde_json::Value) -> Self {
        Self {
            execution_trace,
            input_data,
            output_data: None,
            diff: None,
            telemetry: TelemetrySnapshot {
                metrics: HashMap::new(),
                recent_logs: Vec::new(),
                performance: None,
            },
            additional: HashMap::new(),
        }
    }

    pub fn with_output(mut self, output: serde_json::Value) -> Self {
        self.output_data = Some(output);
        self
    }

    pub fn with_diff(mut self, diff: Diff) -> Self {
        self.diff = Some(diff);
        self
    }

    pub fn with_telemetry(mut self, telemetry: TelemetrySnapshot) -> Self {
        self.telemetry = telemetry;
        self
    }

    /// Injects structured auditing data into the additional metadata map
    pub fn with_auditing_data(mut self, data: AuditingData) -> Self {
        if let Ok(value) = serde_json::to_value(data) {
            self.additional.insert("audit_trail".to_string(), value);
        }
        self
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_auditing_data_integration() {
        // 1. Create a "dummy" trace for the context
        let trace = ExecutionTrace {
            steps: vec![],
            duration_ms: 0,
        };

        // 2. Create your new AuditingData
        let audit = AuditingData {
            intent: "High-Value Trade".to_string(),
            result: "Execute Buy".to_string(),
            relevant_trace_steps: vec!["step_1".to_string()],
            metadata: HashMap::from([
                ("asset".to_string(), json!("SOL")),
                ("amount".to_string(), json!(10.5)),
            ]),
            policy_status: "Pass".to_string(),
        };

        // 3. Use the Builder Pattern to inject it
        let context = ReviewContext::new(trace, json!({})).with_auditing_data(audit);

        // 4. Verify it was stored correctly in the 'additional' HashMap
        let stored_audit = context
            .additional
            .get("audit_trail")
            .expect("Audit trail should exist");

        assert_eq!(stored_audit["intent"], "High-Value Trade");
        assert_eq!(stored_audit["policy_status"], "Pass");

        println!("✅ Audit Trail successfully serialized into ReviewContext!");
    }
}
/// Review metadata (re-exported from types for convenience)
pub use crate::hitl::types::ReviewMetadata;
