//! Parameterized scenario expansion for the MoFA testing framework.
//!
//! This module enables a single scenario template to expand into many concrete
//! test cases by substituting variable placeholders with values from parameter
//! sets.
//!
//! # Overview
//!
//! - [`ParameterSet`]: a named map of variable → value bindings for one test variant.
//! - [`ParameterMatrix`]: generates the Cartesian product of multiple variable dimensions.
//! - [`ParameterizedScenario`]: wraps an [`AgentTestScenario`] template and expands it
//!   against a list of [`ParameterSet`]s.
//! - [`ParameterizedScenarioFile`]: YAML/TOML/JSON-loadable file that bundles a template
//!   scenario and inline parameter sets.
//!
//! Placeholder syntax is `{{variable_name}}` inside `user_input`, `text`, and
//! `pattern` fields of the scenario template.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsl::{
    AgentTestScenario, ScenarioLoadError, ScenarioTurn, TurnExpectation,
};

// ─── Error types ────────────────────────────────────────────────────────────

/// Errors produced during parameterized expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParameterExpansionError {
    /// No parameter sets were provided.
    EmptyParameterSets,
    /// Matrix expansion would produce no cases (some dimension is empty).
    EmptyMatrixDimension { variable: String },
    /// A placeholder in the template has no corresponding variable binding.
    MissingVariable {
        set_name: String,
        variable: String,
    },
    /// Matrix expansion exceeds the configured safety limit.
    MatrixExpansionLimit {
        requested: usize,
        limit: usize,
    },
    /// Scenario validation failed after expansion.
    ValidationFailed(Vec<String>),
    /// File deserialization failed.
    LoadError(ScenarioLoadError),
}

impl Display for ParameterExpansionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyParameterSets => write!(f, "parameter set list is empty"),
            Self::EmptyMatrixDimension { variable } => {
                write!(f, "matrix dimension '{}' has no values", variable)
            }
            Self::MissingVariable { set_name, variable } => {
                write!(
                    f,
                    "parameter set '{}' is missing variable '{}'",
                    set_name, variable
                )
            }
            Self::MatrixExpansionLimit { requested, limit } => {
                write!(
                    f,
                    "matrix expansion would produce {} cases, exceeding limit of {}",
                    requested, limit
                )
            }
            Self::ValidationFailed(errors) => {
                write!(f, "expanded scenario validation failed: {}", errors.join("; "))
            }
            Self::LoadError(err) => write!(f, "parameterized scenario load error: {}", err),
        }
    }
}

impl Error for ParameterExpansionError {}

// ─── ParameterSet ───────────────────────────────────────────────────────────

/// A named set of variable bindings for one parameterized test variant.
///
/// The name is used in expanded test case names for traceability.
/// Variables are substituted into `{{variable}}` placeholders in the template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterSet {
    pub name: String,
    #[serde(flatten)]
    pub variables: BTreeMap<String, String>,
}

impl ParameterSet {
    /// Create a parameter set with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            variables: BTreeMap::new(),
        }
    }

    /// Add a variable binding.
    pub fn with_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(key.into(), value.into());
        self
    }

    /// Look up a variable value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.variables.get(key).map(|s| s.as_str())
    }
}

// ─── ParameterMatrix ────────────────────────────────────────────────────────

/// Default safety limit for Cartesian product expansion.
const DEFAULT_MATRIX_LIMIT: usize = 10_000;

/// Generates [`ParameterSet`]s from the Cartesian product of variable dimensions.
///
/// Each dimension is a variable name mapped to a list of possible values.
/// The product of all dimensions produces the full matrix of test variants.
///
/// # Example
///
/// ```ignore
/// let sets = ParameterMatrix::new()
///     .dimension("city", vec!["Berlin", "Tokyo", "NYC"])
///     .dimension("unit", vec!["celsius", "fahrenheit"])
///     .expand()?;
/// // Produces 6 parameter sets: Berlin×celsius, Berlin×fahrenheit, …
/// ```
#[derive(Debug, Clone)]
pub struct ParameterMatrix {
    /// Insertion-ordered dimensions to preserve deterministic expansion order.
    dimensions: Vec<(String, Vec<String>)>,
    limit: usize,
}

impl Default for ParameterMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl ParameterMatrix {
    /// Create an empty matrix with the default expansion limit.
    pub fn new() -> Self {
        Self {
            dimensions: Vec::new(),
            limit: DEFAULT_MATRIX_LIMIT,
        }
    }

    /// Set the maximum number of expanded cases (default: 10 000).
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Add a dimension (variable with possible values).
    pub fn dimension(
        mut self,
        variable: impl Into<String>,
        values: Vec<impl Into<String>>,
    ) -> Self {
        self.dimensions.push((
            variable.into(),
            values.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Compute the total number of combinations without allocating.
    pub fn combination_count(&self) -> usize {
        self.dimensions
            .iter()
            .map(|(_, vals)| vals.len())
            .product()
    }

    /// Expand the matrix into concrete [`ParameterSet`]s.
    pub fn expand(&self) -> Result<Vec<ParameterSet>, ParameterExpansionError> {
        // Validate dimensions
        for (var, vals) in &self.dimensions {
            if vals.is_empty() {
                return Err(ParameterExpansionError::EmptyMatrixDimension {
                    variable: var.clone(),
                });
            }
        }

        if self.dimensions.is_empty() {
            return Err(ParameterExpansionError::EmptyParameterSets);
        }

        let total = self.combination_count();
        if total > self.limit {
            return Err(ParameterExpansionError::MatrixExpansionLimit {
                requested: total,
                limit: self.limit,
            });
        }

        let mut sets = Vec::with_capacity(total);
        let mut indices = vec![0usize; self.dimensions.len()];

        loop {
            // Build name and variables from current indices
            let name_parts: Vec<String> = self
                .dimensions
                .iter()
                .zip(indices.iter())
                .map(|((_, vals), &idx)| vals[idx].clone())
                .collect();
            let name = name_parts.join("_");

            let mut ps = ParameterSet::new(&name);
            for ((var, vals), &idx) in self.dimensions.iter().zip(indices.iter()) {
                ps.variables.insert(var.clone(), vals[idx].clone());
            }
            sets.push(ps);

            // Increment odometer
            if !increment_indices(&mut indices, &self.dimensions) {
                break;
            }
        }

        Ok(sets)
    }
}

/// Odometer-style increment. Returns false when all combinations exhausted.
fn increment_indices(indices: &mut [usize], dimensions: &[(String, Vec<String>)]) -> bool {
    for i in (0..indices.len()).rev() {
        indices[i] += 1;
        if indices[i] < dimensions[i].1.len() {
            return true;
        }
        indices[i] = 0;
    }
    false
}

// ─── ParameterizedScenario ──────────────────────────────────────────────────

/// A scenario template + parameter sets that expand into multiple concrete scenarios.
#[derive(Debug, Clone)]
pub struct ParameterizedScenario {
    template: AgentTestScenario,
    parameter_sets: Vec<ParameterSet>,
}

impl ParameterizedScenario {
    /// Create a parameterized scenario from a template and parameter sets.
    pub fn new(
        template: AgentTestScenario,
        parameter_sets: Vec<ParameterSet>,
    ) -> Self {
        Self {
            template,
            parameter_sets,
        }
    }

    /// The number of expanded test cases.
    pub fn case_count(&self) -> usize {
        self.parameter_sets.len()
    }

    /// Access the raw parameter sets.
    pub fn parameter_sets(&self) -> &[ParameterSet] {
        &self.parameter_sets
    }

    /// Expand the template into concrete scenarios.
    ///
    /// Each expanded scenario has a unique `agent_id` of the form
    /// `{original_agent_id}[{parameter_set_name}]` for stable identification.
    pub fn expand(&self) -> Result<Vec<AgentTestScenario>, ParameterExpansionError> {
        if self.parameter_sets.is_empty() {
            return Err(ParameterExpansionError::EmptyParameterSets);
        }

        // Collect all placeholders in the template
        let placeholders = collect_placeholders(&self.template);

        let mut expanded = Vec::with_capacity(self.parameter_sets.len());

        for ps in &self.parameter_sets {
            // Validate all placeholders have bindings
            for placeholder in &placeholders {
                if !ps.variables.contains_key(placeholder) {
                    return Err(ParameterExpansionError::MissingVariable {
                        set_name: ps.name.clone(),
                        variable: placeholder.clone(),
                    });
                }
            }

            let scenario = substitute_scenario(&self.template, ps);

            if let Err(err) = scenario.validate() {
                return Err(ParameterExpansionError::ValidationFailed(err.errors));
            }

            expanded.push(scenario);
        }

        Ok(expanded)
    }
}

/// Collect all `{{variable}}` placeholders found in the template.
fn collect_placeholders(scenario: &AgentTestScenario) -> Vec<String> {
    let mut placeholders = Vec::new();
    let re = regex::Regex::new(r"\{\{(\w+)\}\}").expect("placeholder regex is valid");

    // Scan agent_id
    for cap in re.captures_iter(&scenario.agent_id) {
        placeholders.push(cap[1].to_string());
    }

    // Scan system prompt
    if let Some(prompt) = &scenario.system_prompt {
        for cap in re.captures_iter(prompt) {
            placeholders.push(cap[1].to_string());
        }
    }

    // Scan turns
    for turn in &scenario.turns {
        for cap in re.captures_iter(&turn.user_input) {
            placeholders.push(cap[1].to_string());
        }
        for expectation in &turn.expectations {
            match expectation {
                TurnExpectation::RespondContaining { text }
                | TurnExpectation::RespondExact { text } => {
                    for cap in re.captures_iter(text) {
                        placeholders.push(cap[1].to_string());
                    }
                }
                TurnExpectation::RespondMatchingRegex { pattern } => {
                    for cap in re.captures_iter(pattern) {
                        placeholders.push(cap[1].to_string());
                    }
                }
                TurnExpectation::CallTool { name } => {
                    for cap in re.captures_iter(name) {
                        placeholders.push(cap[1].to_string());
                    }
                }
                TurnExpectation::CallToolWith { name, .. } => {
                    for cap in re.captures_iter(name) {
                        placeholders.push(cap[1].to_string());
                    }
                }
                TurnExpectation::NotCallAnyTool => {}
            }
        }
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    placeholders.retain(|p| seen.insert(p.clone()));
    placeholders
}

/// Produce a concrete scenario by replacing `{{variable}}` with values from the parameter set.
fn substitute_scenario(
    template: &AgentTestScenario,
    ps: &ParameterSet,
) -> AgentTestScenario {
    let agent_id = format!(
        "{}[{}]",
        substitute_str(&template.agent_id, ps),
        ps.name
    );

    let system_prompt = template
        .system_prompt
        .as_ref()
        .map(|prompt| substitute_str(prompt, ps));

    let tools = template.tools.clone();

    let turns: Vec<ScenarioTurn> = template
        .turns
        .iter()
        .map(|turn| ScenarioTurn {
            user_input: substitute_str(&turn.user_input, ps),
            expectations: turn
                .expectations
                .iter()
                .map(|exp| substitute_expectation(exp, ps))
                .collect(),
        })
        .collect();

    AgentTestScenario {
        agent_id,
        system_prompt,
        tools,
        turns,
    }
}

/// Replace `{{var}}` occurrences in a string.
fn substitute_str(template: &str, ps: &ParameterSet) -> String {
    let mut result = template.to_string();
    for (key, value) in &ps.variables {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

/// Substitute placeholders inside a single expectation.
fn substitute_expectation(
    exp: &TurnExpectation,
    ps: &ParameterSet,
) -> TurnExpectation {
    match exp {
        TurnExpectation::CallTool { name } => TurnExpectation::CallTool {
            name: substitute_str(name, ps),
        },
        TurnExpectation::CallToolWith { name, arguments } => {
            TurnExpectation::CallToolWith {
                name: substitute_str(name, ps),
                arguments: substitute_json_value(arguments, ps),
            }
        }
        TurnExpectation::NotCallAnyTool => TurnExpectation::NotCallAnyTool,
        TurnExpectation::RespondContaining { text } => {
            TurnExpectation::RespondContaining {
                text: substitute_str(text, ps),
            }
        }
        TurnExpectation::RespondExact { text } => TurnExpectation::RespondExact {
            text: substitute_str(text, ps),
        },
        TurnExpectation::RespondMatchingRegex { pattern } => {
            TurnExpectation::RespondMatchingRegex {
                pattern: substitute_str(pattern, ps),
            }
        }
    }
}

/// Recursively substitute `{{var}}` in JSON string values.
fn substitute_json_value(value: &Value, ps: &ParameterSet) -> Value {
    match value {
        Value::String(s) => Value::String(substitute_str(s, ps)),
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| substitute_json_value(v, ps)).collect())
        }
        Value::Object(obj) => {
            let mut new_obj = serde_json::Map::new();
            for (k, v) in obj {
                new_obj.insert(k.clone(), substitute_json_value(v, ps));
            }
            Value::Object(new_obj)
        }
        other => other.clone(),
    }
}

// ─── File-backed parameterized scenarios ────────────────────────────────────

/// On-disk representation of a parameterized scenario.
///
/// Supports both explicit parameter lists and matrix definitions.
#[derive(Debug, Clone, Deserialize)]
pub struct ParameterizedScenarioFile {
    /// Inline scenario template (same schema as non-parameterized scenario files).
    pub template: ScenarioFileTemplate,
    /// Explicit parameter sets.
    #[serde(default)]
    pub parameters: Vec<ParameterSetFile>,
    /// Matrix-style parameter generation.
    #[serde(default)]
    pub matrix: Option<MatrixFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioFileTemplate {
    pub agent_id: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    pub turns: Vec<ScenarioFileTurnTemplate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioFileTurnTemplate {
    pub user: String,
    #[serde(default, alias = "expectations")]
    pub expect: Vec<ScenarioFileExpectationTemplate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScenarioFileExpectationTemplate {
    CallTool { name: String },
    CallToolWith { name: String, arguments: Value },
    NotCallAnyTool,
    RespondContaining { text: String },
    RespondExact { text: String },
    RespondMatchingRegex { pattern: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParameterSetFile {
    pub name: String,
    #[serde(default, alias = "variables")]
    pub vars: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatrixFile {
    #[serde(default)]
    pub limit: Option<usize>,
    pub dimensions: HashMap<String, Vec<String>>,
}

impl ParameterizedScenarioFile {
    /// Parse from YAML.
    pub fn from_yaml_str(input: &str) -> Result<ParameterizedScenario, ParameterExpansionError> {
        let file: Self = serde_yaml::from_str(input)
            .map_err(|e| ParameterExpansionError::LoadError(ScenarioLoadError::Yaml(e.to_string())))?;
        file.into_parameterized()
    }

    /// Parse from TOML.
    pub fn from_toml_str(input: &str) -> Result<ParameterizedScenario, ParameterExpansionError> {
        let file: Self = toml::from_str(input)
            .map_err(|e| ParameterExpansionError::LoadError(ScenarioLoadError::Toml(e.to_string())))?;
        file.into_parameterized()
    }

    /// Parse from JSON.
    pub fn from_json_str(input: &str) -> Result<ParameterizedScenario, ParameterExpansionError> {
        let file: Self = serde_json::from_str(input)
            .map_err(|e| ParameterExpansionError::LoadError(ScenarioLoadError::Json(e.to_string())))?;
        file.into_parameterized()
    }

    fn into_parameterized(self) -> Result<ParameterizedScenario, ParameterExpansionError> {
        let template = self.to_scenario_template();

        let mut parameter_sets: Vec<ParameterSet> = self
            .parameters
            .into_iter()
            .map(|pf| {
                let mut ps = ParameterSet::new(&pf.name);
                ps.variables = pf.vars.into_iter().collect();
                ps
            })
            .collect();

        // If a matrix is defined, expand it and append to the explicit sets
        if let Some(matrix_file) = self.matrix {
            let mut matrix = ParameterMatrix::new();
            if let Some(limit) = matrix_file.limit {
                matrix = matrix.with_limit(limit);
            }
            // Sort dimension keys for deterministic ordering
            let mut dim_keys: Vec<String> = matrix_file.dimensions.keys().cloned().collect();
            dim_keys.sort();
            for key in dim_keys {
                let values = matrix_file.dimensions[&key].clone();
                matrix = matrix.dimension(key, values);
            }
            let matrix_sets = matrix.expand()?;
            parameter_sets.extend(matrix_sets);
        }

        Ok(ParameterizedScenario::new(template, parameter_sets))
    }

    fn to_scenario_template(&self) -> AgentTestScenario {
        AgentTestScenario {
            agent_id: self.template.agent_id.clone(),
            system_prompt: self.template.system_prompt.clone(),
            tools: self.template.tools.clone(),
            turns: self
                .template
                .turns
                .iter()
                .map(|turn| ScenarioTurn {
                    user_input: turn.user.clone(),
                    expectations: turn
                        .expect
                        .iter()
                        .map(|exp| match exp {
                            ScenarioFileExpectationTemplate::CallTool { name } => {
                                TurnExpectation::CallTool { name: name.clone() }
                            }
                            ScenarioFileExpectationTemplate::CallToolWith {
                                name,
                                arguments,
                            } => TurnExpectation::CallToolWith {
                                name: name.clone(),
                                arguments: arguments.clone(),
                            },
                            ScenarioFileExpectationTemplate::NotCallAnyTool => {
                                TurnExpectation::NotCallAnyTool
                            }
                            ScenarioFileExpectationTemplate::RespondContaining { text } => {
                                TurnExpectation::RespondContaining { text: text.clone() }
                            }
                            ScenarioFileExpectationTemplate::RespondExact { text } => {
                                TurnExpectation::RespondExact { text: text.clone() }
                            }
                            ScenarioFileExpectationTemplate::RespondMatchingRegex {
                                pattern,
                            } => TurnExpectation::RespondMatchingRegex {
                                pattern: pattern.clone(),
                            },
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}
