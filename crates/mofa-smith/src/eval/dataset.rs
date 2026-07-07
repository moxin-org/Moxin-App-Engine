//! Evaluation dataset — a collection of test cases loaded from YAML or JSON.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A single evaluation test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    /// Unique identifier for this case.
    pub id: String,
    /// Human-readable description of what is being tested.
    pub description: String,
    /// Named inputs passed to the swarm executor.
    #[serde(default)]
    pub inputs: HashMap<String, String>,
    /// Expected output or keyword(s) the scorer checks against.
    pub expected_output: Option<String>,
    /// Optional tags for filtering and grouping.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl EvalCase {
    /// Create a new case with the given id and description.
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            inputs: HashMap::new(),
            expected_output: None,
            tags: Vec::new(),
        }
    }

    /// Add an input key-value pair.
    pub fn with_input(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inputs.insert(key.into(), value.into());
        self
    }

    /// Set the expected output.
    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected_output = Some(expected.into());
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Return all input values joined into a single string (used by scorers).
    pub fn inputs_as_text(&self) -> String {
        self.inputs.values().cloned().collect::<Vec<_>>().join(" ")
    }
}

/// A named collection of [`EvalCase`]s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalDataset {
    /// Dataset name shown in reports.
    pub name: String,
    /// Optional description of what this dataset covers.
    pub description: Option<String>,
    /// The test cases.
    pub cases: Vec<EvalCase>,
}

impl EvalDataset {
    /// Create an empty dataset with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            cases: Vec::new(),
        }
    }

    /// Add a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a case.
    pub fn with_case(mut self, case: EvalCase) -> Self {
        self.cases.push(case);
        self
    }

    /// Load from a YAML file.
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading dataset file: {}", path.display()))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("parsing YAML dataset: {}", path.display()))
    }

    /// Load from a JSON file.
    pub fn from_json_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading dataset file: {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("parsing JSON dataset: {}", path.display()))
    }

    /// Auto-detect format from file extension and load.
    pub fn load(path: &Path) -> Result<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml") | Some("yml") => Self::from_yaml_file(path),
            Some("json") => Self::from_json_file(path),
            other => anyhow::bail!(
                "unsupported dataset file extension: {:?} (use .yaml or .json)",
                other
            ),
        }
    }

    /// Number of cases in the dataset.
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// True if there are no cases.
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_case_builder() {
        let case = EvalCase::new("c1", "test case")
            .with_input("doc", "revenue was 1.2M")
            .with_expected("1.2M")
            .with_tag("finance");

        assert_eq!(case.id, "c1");
        assert_eq!(case.inputs["doc"], "revenue was 1.2M");
        assert_eq!(case.expected_output.as_deref(), Some("1.2M"));
        assert_eq!(case.tags, vec!["finance"]);
    }

    #[test]
    fn test_inputs_as_text_joins_values() {
        let case = EvalCase::new("c1", "test")
            .with_input("a", "hello")
            .with_input("b", "world");

        let text = case.inputs_as_text();
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
    }

    #[test]
    fn test_dataset_builder() {
        let ds = EvalDataset::new("my-dataset")
            .with_description("test dataset")
            .with_case(EvalCase::new("c1", "first case"));

        assert_eq!(ds.name, "my-dataset");
        assert_eq!(ds.len(), 1);
        assert!(!ds.is_empty());
    }
}
