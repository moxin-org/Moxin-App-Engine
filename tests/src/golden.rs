//! Golden response testing for the MoFA testing framework.
//!
//! This module provides snapshot-based testing: record agent outputs as golden
//! baselines, then compare future runs against them to detect regressions.
//!
//! # Overview
//!
//! - [`GoldenSnapshot`]: serializable record of one scenario's turn outputs.
//! - [`GoldenStore`]: reads/writes golden snapshot files from a directory.
//! - [`GoldenCompareMode`]: strict validation vs. update mode.
//! - [`GoldenDiff`]: structured per-field diff when actual output diverges.
//! - [`GoldenResult`]: per-turn comparison outcome.
//! - [`Normalizer`]: pluggable text normalization to ignore non-deterministic fragments.
//! - [`run_golden_test`]: captures scenario output and compares against stored golden.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsl::{ScenarioAgent, ScenarioTurnOutput, ToolCallRecord};
use crate::report::{TestCaseResult, TestReport, TestReportBuilder, TestStatus};

// ─── Error types ────────────────────────────────────────────────────────────

/// Errors produced during golden test operations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoldenError {
    /// Snapshot file could not be read.
    ReadFailed(String),
    /// Snapshot file could not be written.
    WriteFailed(String),
    /// Snapshot file contains invalid data.
    ParseFailed(String),
    /// Golden snapshot not found and mode is strict.
    SnapshotNotFound { test_name: String },
    /// Actual output diverges from golden baseline.
    Mismatch {
        test_name: String,
        diffs: Vec<GoldenDiff>,
    },
}

impl Display for GoldenError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadFailed(msg) => write!(f, "golden snapshot read failed: {msg}"),
            Self::WriteFailed(msg) => write!(f, "golden snapshot write failed: {msg}"),
            Self::ParseFailed(msg) => write!(f, "golden snapshot parse failed: {msg}"),
            Self::SnapshotNotFound { test_name } => {
                write!(f, "golden snapshot not found for test '{test_name}'")
            }
            Self::Mismatch { test_name, diffs } => {
                let diff_msgs: Vec<String> = diffs.iter().map(|d| d.to_string()).collect();
                write!(
                    f,
                    "golden mismatch for '{}': {}",
                    test_name,
                    diff_msgs.join("; ")
                )
            }
        }
    }
}

impl Error for GoldenError {}

// ─── GoldenDiff ─────────────────────────────────────────────────────────────

/// Describes a single field-level difference between actual and golden output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GoldenDiff {
    /// Turn count differs.
    TurnCountMismatch {
        expected: usize,
        actual: usize,
    },
    /// Response text differs for a specific turn.
    ResponseMismatch {
        turn: usize,
        expected: String,
        actual: String,
    },
    /// Tool call count differs for a specific turn.
    ToolCallCountMismatch {
        turn: usize,
        expected: usize,
        actual: usize,
    },
    /// Tool call at a specific position differs.
    ToolCallMismatch {
        turn: usize,
        index: usize,
        expected_name: String,
        actual_name: String,
    },
    /// Tool call arguments differ.
    ToolCallArgsMismatch {
        turn: usize,
        tool_name: String,
        expected: Value,
        actual: Value,
    },
}

impl Display for GoldenDiff {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TurnCountMismatch { expected, actual } => {
                write!(f, "turn count: expected {expected}, got {actual}")
            }
            Self::ResponseMismatch {
                turn,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "turn {turn} response: expected \"{expected}\", got \"{actual}\""
                )
            }
            Self::ToolCallCountMismatch {
                turn,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "turn {turn} tool call count: expected {expected}, got {actual}"
                )
            }
            Self::ToolCallMismatch {
                turn,
                index,
                expected_name,
                actual_name,
            } => {
                write!(
                    f,
                    "turn {turn} tool[{index}]: expected \"{expected_name}\", got \"{actual_name}\""
                )
            }
            Self::ToolCallArgsMismatch {
                turn,
                tool_name,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "turn {turn} tool \"{tool_name}\" args: expected {expected}, got {actual}"
                )
            }
        }
    }
}

// ─── GoldenSnapshot ─────────────────────────────────────────────────────────

/// A golden snapshot recording the expected outputs for each turn in a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenSnapshot {
    /// Identifier matching the scenario's agent_id.
    pub test_name: String,
    /// Recorded turn outputs.
    pub turns: Vec<GoldenTurnSnapshot>,
    /// Optional metadata (e.g. creation timestamp, model version).
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Golden record for a single turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenTurnSnapshot {
    pub user_input: String,
    pub response: String,
    pub tool_calls: Vec<ToolCallRecord>,
}

impl GoldenSnapshot {
    /// Create a snapshot from a test name and collected turn outputs.
    pub fn new(
        test_name: impl Into<String>,
        turns: Vec<GoldenTurnSnapshot>,
    ) -> Self {
        Self {
            test_name: test_name.into(),
            turns,
            metadata: BTreeMap::new(),
        }
    }

    /// Attach metadata (e.g. model version, timestamp).
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, GoldenError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| GoldenError::WriteFailed(e.to_string()))
    }

    /// Deserialize from JSON.
    pub fn from_json(input: &str) -> Result<Self, GoldenError> {
        serde_json::from_str(input)
            .map_err(|e| GoldenError::ParseFailed(e.to_string()))
    }

    /// Serialize to YAML.
    pub fn to_yaml(&self) -> Result<String, GoldenError> {
        serde_yaml::to_string(self)
            .map_err(|e| GoldenError::WriteFailed(e.to_string()))
    }

    /// Deserialize from YAML.
    pub fn from_yaml(input: &str) -> Result<Self, GoldenError> {
        serde_yaml::from_str(input)
            .map_err(|e| GoldenError::ParseFailed(e.to_string()))
    }
}

// ─── Normalizer ─────────────────────────────────────────────────────────────

/// Pluggable text normalizer for stripping non-deterministic fragments.
///
/// Apply normalizers before comparison so that timestamps, UUIDs, and other
/// volatile content don't cause false mismatches.
pub trait Normalizer: Send + Sync {
    /// Transform a response string into its normalized form.
    fn normalize(&self, input: &str) -> String;
}

/// Strips leading/trailing whitespace and collapses internal runs of whitespace.
#[derive(Debug, Clone, Default)]
pub struct WhitespaceNormalizer;

impl Normalizer for WhitespaceNormalizer {
    fn normalize(&self, input: &str) -> String {
        input.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// Replaces patterns matching a regex with a fixed placeholder.
#[derive(Debug, Clone)]
pub struct RegexNormalizer {
    pattern: regex::Regex,
    replacement: String,
}

impl RegexNormalizer {
    /// Create a normalizer that replaces all matches of `pattern` with `replacement`.
    pub fn new(pattern: &str, replacement: impl Into<String>) -> Result<Self, GoldenError> {
        let regex = regex::Regex::new(pattern)
            .map_err(|e| GoldenError::ParseFailed(format!("invalid normalizer regex: {e}")))?;
        Ok(Self {
            pattern: regex,
            replacement: replacement.into(),
        })
    }
}

impl Normalizer for RegexNormalizer {
    fn normalize(&self, input: &str) -> String {
        self.pattern.replace_all(input, &self.replacement).to_string()
    }
}

/// Chains multiple normalizers in sequence.
#[derive(Default)]
pub struct NormalizerChain {
    normalizers: Vec<Box<dyn Normalizer>>,
}

impl NormalizerChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(mut self, normalizer: impl Normalizer + 'static) -> Self {
        self.normalizers.push(Box::new(normalizer));
        self
    }

    /// Build a chain with common normalizers (whitespace + UUID + ISO timestamp).
    pub fn default_chain() -> Result<Self, GoldenError> {
        Ok(Self::new()
            .add(WhitespaceNormalizer)
            .add(RegexNormalizer::new(
                r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
                "<UUID>",
            )?)
            .add(RegexNormalizer::new(
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}",
                "<TIMESTAMP>",
            )?))
    }
}

impl Normalizer for NormalizerChain {
    fn normalize(&self, input: &str) -> String {
        let mut result = input.to_string();
        for n in &self.normalizers {
            result = n.normalize(&result);
        }
        result
    }
}

// ─── GoldenStore ────────────────────────────────────────────────────────────

/// Filesystem-backed store for golden snapshot files.
///
/// Each snapshot is stored as `{test_name}.golden.json` inside the store's
/// root directory.
pub struct GoldenStore {
    root: PathBuf,
}

impl GoldenStore {
    /// Create a store rooted at the given directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Path to the snapshot file for a given test name.
    pub fn snapshot_path(&self, test_name: &str) -> PathBuf {
        let safe_name = test_name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect::<String>();
        self.root.join(format!("{safe_name}.golden.json"))
    }

    /// Check if a golden snapshot exists for the given test name.
    pub fn exists(&self, test_name: &str) -> bool {
        self.snapshot_path(test_name).exists()
    }

    /// Load a golden snapshot from disk.
    pub fn load(&self, test_name: &str) -> Result<GoldenSnapshot, GoldenError> {
        let path = self.snapshot_path(test_name);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| GoldenError::ReadFailed(format!("{}: {e}", path.display())))?;
        GoldenSnapshot::from_json(&content)
    }

    /// Save a golden snapshot to disk (creates parent directories).
    pub fn save(&self, snapshot: &GoldenSnapshot) -> Result<PathBuf, GoldenError> {
        let path = self.snapshot_path(&snapshot.test_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| GoldenError::WriteFailed(format!("mkdir {}: {e}", parent.display())))?;
        }
        let json = snapshot.to_json()?;
        std::fs::write(&path, &json)
            .map_err(|e| GoldenError::WriteFailed(format!("{}: {e}", path.display())))?;
        Ok(path)
    }

    /// List all golden snapshot files in the store.
    pub fn list(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.ends_with(".golden.json") {
                    names.push(file_name.trim_end_matches(".golden.json").to_string());
                }
            }
        }
        names.sort();
        names
    }
}

// ─── Comparison ─────────────────────────────────────────────────────────────

/// Compare mode for golden tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoldenCompareMode {
    /// Strict: fail if actual differs from golden.
    Strict,
    /// Update: overwrite golden with actual output (for baseline refresh).
    Update,
}

/// Result of comparing actual output against a golden snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct GoldenCompareResult {
    pub test_name: String,
    pub mode: GoldenCompareMode,
    pub passed: bool,
    pub diffs: Vec<GoldenDiff>,
    /// Set to true if the golden was updated (only in Update mode).
    pub updated: bool,
}

/// Compare actual turn outputs against a golden snapshot.
///
/// Optionally applies a `Normalizer` to response text before comparison.
pub fn compare_golden(
    golden: &GoldenSnapshot,
    actual_turns: &[(String, ScenarioTurnOutput)],
    normalizer: Option<&dyn Normalizer>,
) -> Vec<GoldenDiff> {
    let mut diffs = Vec::new();

    if golden.turns.len() != actual_turns.len() {
        diffs.push(GoldenDiff::TurnCountMismatch {
            expected: golden.turns.len(),
            actual: actual_turns.len(),
        });
        return diffs;
    }

    for (idx, (golden_turn, (_user_input, actual_output))) in
        golden.turns.iter().zip(actual_turns.iter()).enumerate()
    {
        let turn_num = idx + 1;

        // Compare response text
        let expected_response = match normalizer {
            Some(n) => n.normalize(&golden_turn.response),
            None => golden_turn.response.clone(),
        };
        let actual_response = match normalizer {
            Some(n) => n.normalize(&actual_output.response),
            None => actual_output.response.clone(),
        };

        if expected_response != actual_response {
            diffs.push(GoldenDiff::ResponseMismatch {
                turn: turn_num,
                expected: golden_turn.response.clone(),
                actual: actual_output.response.clone(),
            });
        }

        // Compare tool call count
        if golden_turn.tool_calls.len() != actual_output.tool_calls.len() {
            diffs.push(GoldenDiff::ToolCallCountMismatch {
                turn: turn_num,
                expected: golden_turn.tool_calls.len(),
                actual: actual_output.tool_calls.len(),
            });
            continue;
        }

        // Compare tool calls by position
        for (tc_idx, (expected_tc, actual_tc)) in golden_turn
            .tool_calls
            .iter()
            .zip(actual_output.tool_calls.iter())
            .enumerate()
        {
            if expected_tc.name != actual_tc.name {
                diffs.push(GoldenDiff::ToolCallMismatch {
                    turn: turn_num,
                    index: tc_idx,
                    expected_name: expected_tc.name.clone(),
                    actual_name: actual_tc.name.clone(),
                });
            } else if expected_tc.arguments != actual_tc.arguments {
                diffs.push(GoldenDiff::ToolCallArgsMismatch {
                    turn: turn_num,
                    tool_name: expected_tc.name.clone(),
                    expected: expected_tc.arguments.clone(),
                    actual: actual_tc.arguments.clone(),
                });
            }
        }
    }

    diffs
}

// ─── Golden test runner ─────────────────────────────────────────────────────

/// Configuration for running a golden test.
pub struct GoldenTestConfig {
    /// Store for reading/writing snapshots.
    pub store: GoldenStore,
    /// Comparison mode.
    pub mode: GoldenCompareMode,
    /// Optional normalizer for response text.
    pub normalizer: Option<Box<dyn Normalizer>>,
}

impl GoldenTestConfig {
    /// Create a config in strict comparison mode.
    pub fn strict(store: GoldenStore) -> Self {
        Self {
            store,
            mode: GoldenCompareMode::Strict,
            normalizer: None,
        }
    }

    /// Create a config in update mode (rewrites goldens).
    pub fn update(store: GoldenStore) -> Self {
        Self {
            store,
            mode: GoldenCompareMode::Update,
            normalizer: None,
        }
    }

    /// Attach a normalizer.
    pub fn with_normalizer(mut self, normalizer: impl Normalizer + 'static) -> Self {
        self.normalizer = Some(Box::new(normalizer));
        self
    }
}

/// Run a golden test for a scenario.
///
/// 1. Executes all turns of the scenario with the given agent.
/// 2. In **Update** mode: saves the outputs as the new golden baseline.
/// 3. In **Strict** mode: compares against the stored golden and reports diffs.
///
/// Returns a `TestReport` with golden comparison results.
pub async fn run_golden_test<A: ScenarioAgent>(
    config: &GoldenTestConfig,
    scenario: &crate::dsl::AgentTestScenario,
    agent: &mut A,
) -> TestReport {
    let test_name = &scenario.agent_id;
    let mut builder = TestReportBuilder::new(format!("golden:{test_name}"));

    // 1. Execute all turns and collect outputs
    let mut turn_outputs: Vec<(String, ScenarioTurnOutput)> = Vec::new();
    let mut execution_failed = false;

    for turn in &scenario.turns {
        let result = agent
            .execute_turn(scenario.system_prompt.as_deref(), &turn.user_input)
            .await;

        match result {
            Ok(output) => {
                turn_outputs.push((turn.user_input.clone(), output));
            }
            Err(err) => {
                builder = builder.add_result(TestCaseResult {
                    name: format!("golden_execution_{test_name}"),
                    status: TestStatus::Failed,
                    duration: std::time::Duration::from_secs(0),
                    error: Some(format!("agent execution failed: {err}")),
                    metadata: vec![],
                });
                execution_failed = true;
                break;
            }
        }
    }

    if execution_failed {
        return builder.build();
    }

    // 2. Build snapshot from actual outputs
    let actual_snapshot = GoldenSnapshot::new(
        test_name.clone(),
        turn_outputs
            .iter()
            .map(|(user_input, output)| GoldenTurnSnapshot {
                user_input: user_input.clone(),
                response: output.response.clone(),
                tool_calls: output.tool_calls.clone(),
            })
            .collect(),
    );

    match config.mode {
        GoldenCompareMode::Update => {
            // Save the snapshot as the new golden baseline
            match config.store.save(&actual_snapshot) {
                Ok(path) => {
                    builder = builder.add_result(TestCaseResult {
                        name: format!("golden_update_{test_name}"),
                        status: TestStatus::Passed,
                        duration: std::time::Duration::from_secs(0),
                        error: None,
                        metadata: vec![
                            ("mode".to_string(), "update".to_string()),
                            ("snapshot_path".to_string(), path.display().to_string()),
                            ("turn_count".to_string(), turn_outputs.len().to_string()),
                        ],
                    });
                }
                Err(err) => {
                    builder = builder.add_result(TestCaseResult {
                        name: format!("golden_update_{test_name}"),
                        status: TestStatus::Failed,
                        duration: std::time::Duration::from_secs(0),
                        error: Some(err.to_string()),
                        metadata: vec![("mode".to_string(), "update".to_string())],
                    });
                }
            }
        }
        GoldenCompareMode::Strict => {
            // Load existing golden and compare
            match config.store.load(test_name) {
                Ok(golden) => {
                    let diffs = compare_golden(
                        &golden,
                        &turn_outputs,
                        config.normalizer.as_deref(),
                    );

                    if diffs.is_empty() {
                        builder = builder.add_result(TestCaseResult {
                            name: format!("golden_compare_{test_name}"),
                            status: TestStatus::Passed,
                            duration: std::time::Duration::from_secs(0),
                            error: None,
                            metadata: vec![
                                ("mode".to_string(), "strict".to_string()),
                                ("turn_count".to_string(), turn_outputs.len().to_string()),
                            ],
                        });
                    } else {
                        let diff_msgs: Vec<String> =
                            diffs.iter().map(|d| d.to_string()).collect();
                        builder = builder.add_result(TestCaseResult {
                            name: format!("golden_compare_{test_name}"),
                            status: TestStatus::Failed,
                            duration: std::time::Duration::from_secs(0),
                            error: Some(diff_msgs.join("\n")),
                            metadata: vec![
                                ("mode".to_string(), "strict".to_string()),
                                ("diff_count".to_string(), diffs.len().to_string()),
                            ],
                        });
                    }
                }
                Err(_) => {
                    builder = builder.add_result(TestCaseResult {
                        name: format!("golden_compare_{test_name}"),
                        status: TestStatus::Failed,
                        duration: std::time::Duration::from_secs(0),
                        error: Some(format!(
                            "golden snapshot not found for '{test_name}'. Run with update mode to create baseline."
                        )),
                        metadata: vec![("mode".to_string(), "strict".to_string())],
                    });
                }
            }
        }
    }

    builder.build()
}
