//! Semantic assertion matchers for intent-level validation.
//!
//! This module provides assertion primitives that validate **meaning and intent**
//! rather than exact text, making tests resilient to harmless wording changes
//! while maintaining strict policy checks.
//!
//! # Overview
//!
//! - [`SemanticMatcher`]: trait for all semantic assertion types.
//! - [`ContainsAllFactsMatcher`]: validates that all required facts appear.
//! - [`ExcludesContentMatcher`]: validates that prohibited content is absent.
//! - [`IntentMatcher`]: keyword/pattern-based intent classification.
//! - [`SimilarityMatcher`]: token-overlap similarity with configurable threshold.
//! - [`SemanticAssertionSet`]: bundles multiple matchers for a single response.
//! - [`SemanticMatchResult`]: per-matcher outcome with confidence and explanation.
//! - [`SemanticReport`]: aggregated pass/fail across all matchers.
//!
//! All matchers are deterministic and require no external API calls, making
//! them safe for offline environments and CI pipelines.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

// ─── Error types ────────────────────────────────────────────────────────────

/// Errors produced during semantic assertion configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SemanticAssertionError {
    /// A regex pattern in a matcher is invalid.
    InvalidPattern(String),
    /// Threshold value is out of bounds.
    InvalidThreshold { value: f64, min: f64, max: f64 },
    /// No matchers were provided.
    EmptyMatcherSet,
}

impl Display for SemanticAssertionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPattern(msg) => write!(f, "invalid semantic pattern: {msg}"),
            Self::InvalidThreshold { value, min, max } => {
                write!(
                    f,
                    "threshold {value} is out of bounds [{min}, {max}]"
                )
            }
            Self::EmptyMatcherSet => write!(f, "semantic assertion set has no matchers"),
        }
    }
}

impl Error for SemanticAssertionError {}

// ─── SemanticMatchResult ────────────────────────────────────────────────────

/// Outcome of a single semantic matcher evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticMatchResult {
    /// Name of the matcher that produced this result.
    pub matcher_name: String,
    /// Whether the assertion passed.
    pub passed: bool,
    /// Confidence score in [0.0, 1.0] (1.0 = highest confidence).
    pub confidence: f64,
    /// Human-readable explanation of the result.
    pub explanation: String,
}

impl SemanticMatchResult {
    /// Create a passing result.
    pub fn pass(name: impl Into<String>, confidence: f64, explanation: impl Into<String>) -> Self {
        Self {
            matcher_name: name.into(),
            passed: true,
            confidence,
            explanation: explanation.into(),
        }
    }

    /// Create a failing result.
    pub fn fail(name: impl Into<String>, confidence: f64, explanation: impl Into<String>) -> Self {
        Self {
            matcher_name: name.into(),
            passed: false,
            confidence,
            explanation: explanation.into(),
        }
    }
}

// ─── SemanticMatcher trait ──────────────────────────────────────────────────

/// Trait for semantic assertion matchers.
///
/// Each matcher evaluates a response string and produces a
/// [`SemanticMatchResult`] with a confidence score and explanation.
pub trait SemanticMatcher: Send + Sync {
    /// Unique name identifying this matcher type.
    fn name(&self) -> &str;

    /// Evaluate the response and return a match result.
    fn evaluate(&self, response: &str) -> SemanticMatchResult;
}

// ─── ContainsAllFactsMatcher ────────────────────────────────────────────────

/// Validates that all required facts (keywords/phrases) appear in the response.
///
/// Each fact is checked case-insensitively. The confidence score reflects
/// the fraction of facts found.
///
/// # Example
///
/// ```ignore
/// let matcher = ContainsAllFactsMatcher::new(vec!["Berlin", "22C", "sunny"]);
/// let result = matcher.evaluate("The weather in Berlin is 22C and sunny.");
/// assert!(result.passed);
/// ```
#[derive(Debug, Clone)]
pub struct ContainsAllFactsMatcher {
    facts: Vec<String>,
}

impl ContainsAllFactsMatcher {
    /// Create a matcher requiring all given facts to be present.
    pub fn new(facts: Vec<impl Into<String>>) -> Self {
        Self {
            facts: facts.into_iter().map(Into::into).collect(),
        }
    }
}

impl SemanticMatcher for ContainsAllFactsMatcher {
    fn name(&self) -> &str {
        "contains_all_facts"
    }

    fn evaluate(&self, response: &str) -> SemanticMatchResult {
        if self.facts.is_empty() {
            return SemanticMatchResult::pass(self.name(), 1.0, "no facts to check");
        }

        let response_lower = response.to_lowercase();
        let mut found = Vec::new();
        let mut missing = Vec::new();

        for fact in &self.facts {
            if response_lower.contains(&fact.to_lowercase()) {
                found.push(fact.as_str());
            } else {
                missing.push(fact.as_str());
            }
        }

        let confidence = found.len() as f64 / self.facts.len() as f64;

        if missing.is_empty() {
            SemanticMatchResult::pass(
                self.name(),
                confidence,
                format!("all {} facts found in response", self.facts.len()),
            )
        } else {
            SemanticMatchResult::fail(
                self.name(),
                confidence,
                format!(
                    "missing facts: [{}] (found {}/{})",
                    missing.join(", "),
                    found.len(),
                    self.facts.len()
                ),
            )
        }
    }
}

// ─── ExcludesContentMatcher ─────────────────────────────────────────────────

/// Validates that prohibited content does NOT appear in the response.
///
/// Each banned term is checked case-insensitively. Useful for policy and
/// safety assertions (e.g. no PII, no harmful content).
///
/// # Example
///
/// ```ignore
/// let matcher = ExcludesContentMatcher::new(vec!["password", "SSN", "credit card"]);
/// let result = matcher.evaluate("Here is your account summary.");
/// assert!(result.passed);
/// ```
#[derive(Debug, Clone)]
pub struct ExcludesContentMatcher {
    banned: Vec<String>,
}

impl ExcludesContentMatcher {
    /// Create a matcher that fails if any banned term is found.
    pub fn new(banned: Vec<impl Into<String>>) -> Self {
        Self {
            banned: banned.into_iter().map(Into::into).collect(),
        }
    }
}

impl SemanticMatcher for ExcludesContentMatcher {
    fn name(&self) -> &str {
        "excludes_content"
    }

    fn evaluate(&self, response: &str) -> SemanticMatchResult {
        if self.banned.is_empty() {
            return SemanticMatchResult::pass(self.name(), 1.0, "no banned terms to check");
        }

        let response_lower = response.to_lowercase();
        let mut violations: Vec<&str> = Vec::new();

        for term in &self.banned {
            if response_lower.contains(&term.to_lowercase()) {
                violations.push(term.as_str());
            }
        }

        if violations.is_empty() {
            SemanticMatchResult::pass(
                self.name(),
                1.0,
                format!("none of {} banned terms found", self.banned.len()),
            )
        } else {
            let safe_count = self.banned.len() - violations.len();
            let confidence = safe_count as f64 / self.banned.len() as f64;
            SemanticMatchResult::fail(
                self.name(),
                confidence,
                format!(
                    "prohibited content found: [{}]",
                    violations.join(", ")
                ),
            )
        }
    }
}

// ─── IntentMatcher ──────────────────────────────────────────────────────────

/// Validates that a response matches an expected intent via keyword groups.
///
/// An intent is defined by a name and a set of indicator keywords/phrases.
/// If any indicator is found in the response (case-insensitive), the intent
/// is considered matched. Multiple intents can be checked.
///
/// # Example
///
/// ```ignore
/// let matcher = IntentMatcher::new()
///     .expect_intent("greeting", vec!["hello", "hi", "welcome"])
///     .expect_intent("farewell", vec!["goodbye", "bye", "see you"]);
///
/// let result = matcher.evaluate("Hello! How can I help you?");
/// // Passes if at least one expected intent is matched
/// ```
#[derive(Debug, Clone)]
pub struct IntentMatcher {
    expected_intents: Vec<IntentDefinition>,
    /// If true, ALL expected intents must match. If false, at least one.
    require_all: bool,
}

#[derive(Debug, Clone)]
struct IntentDefinition {
    name: String,
    indicators: Vec<String>,
}

impl IntentMatcher {
    /// Create a new intent matcher (default: require at least one intent).
    pub fn new() -> Self {
        Self {
            expected_intents: Vec::new(),
            require_all: false,
        }
    }

    /// Require ALL expected intents to match (default: at least one).
    pub fn require_all(mut self) -> Self {
        self.require_all = true;
        self
    }

    /// Add an expected intent with indicator keywords.
    pub fn expect_intent(
        mut self,
        name: impl Into<String>,
        indicators: Vec<impl Into<String>>,
    ) -> Self {
        self.expected_intents.push(IntentDefinition {
            name: name.into(),
            indicators: indicators.into_iter().map(Into::into).collect(),
        });
        self
    }

    fn check_intent(&self, intent: &IntentDefinition, response_lower: &str) -> bool {
        intent
            .indicators
            .iter()
            .any(|kw| response_lower.contains(&kw.to_lowercase()))
    }
}

impl Default for IntentMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticMatcher for IntentMatcher {
    fn name(&self) -> &str {
        "intent_match"
    }

    fn evaluate(&self, response: &str) -> SemanticMatchResult {
        if self.expected_intents.is_empty() {
            return SemanticMatchResult::pass(self.name(), 1.0, "no intents to check");
        }

        let response_lower = response.to_lowercase();
        let mut matched: Vec<&str> = Vec::new();
        let mut unmatched: Vec<&str> = Vec::new();

        for intent in &self.expected_intents {
            if self.check_intent(intent, &response_lower) {
                matched.push(&intent.name);
            } else {
                unmatched.push(&intent.name);
            }
        }

        let confidence = matched.len() as f64 / self.expected_intents.len() as f64;

        let passed = if self.require_all {
            unmatched.is_empty()
        } else {
            !matched.is_empty()
        };

        if passed {
            SemanticMatchResult::pass(
                self.name(),
                confidence,
                format!(
                    "matched intents: [{}]",
                    matched.join(", ")
                ),
            )
        } else {
            SemanticMatchResult::fail(
                self.name(),
                confidence,
                format!(
                    "unmatched intents: [{}] (matched: [{}])",
                    unmatched.join(", "),
                    matched.join(", ")
                ),
            )
        }
    }
}

// ─── SimilarityMatcher ──────────────────────────────────────────────────────

/// Validates response similarity to a reference text using token overlap.
///
/// Uses Jaccard similarity on word-level tokens (case-insensitive).
/// This is a deterministic, offline-safe approximation of semantic similarity.
///
/// # Example
///
/// ```ignore
/// let matcher = SimilarityMatcher::new("The temperature is 22 degrees Celsius", 0.5)?;
/// let result = matcher.evaluate("It's about 22 degrees today in Celsius");
/// assert!(result.passed); // sufficient token overlap
/// ```
#[derive(Debug, Clone)]
pub struct SimilarityMatcher {
    reference: String,
    reference_tokens: HashSet<String>,
    threshold: f64,
}

impl SimilarityMatcher {
    /// Create a matcher comparing against a reference text.
    ///
    /// `threshold` must be in `[0.0, 1.0]`.
    pub fn new(
        reference: impl Into<String>,
        threshold: f64,
    ) -> Result<Self, SemanticAssertionError> {
        if !(0.0..=1.0).contains(&threshold) {
            return Err(SemanticAssertionError::InvalidThreshold {
                value: threshold,
                min: 0.0,
                max: 1.0,
            });
        }

        let reference = reference.into();
        let reference_tokens = tokenize(&reference);

        Ok(Self {
            reference,
            reference_tokens,
            threshold,
        })
    }

    /// Compute Jaccard similarity between two token sets.
    fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        let intersection = a.intersection(b).count() as f64;
        let union = a.union(b).count() as f64;
        if union == 0.0 {
            0.0
        } else {
            intersection / union
        }
    }
}

impl SemanticMatcher for SimilarityMatcher {
    fn name(&self) -> &str {
        "similarity"
    }

    fn evaluate(&self, response: &str) -> SemanticMatchResult {
        let response_tokens = tokenize(response);
        let similarity = Self::jaccard_similarity(&self.reference_tokens, &response_tokens);

        if similarity >= self.threshold {
            SemanticMatchResult::pass(
                self.name(),
                similarity,
                format!(
                    "similarity {:.2} >= threshold {:.2}",
                    similarity, self.threshold
                ),
            )
        } else {
            SemanticMatchResult::fail(
                self.name(),
                similarity,
                format!(
                    "similarity {:.2} < threshold {:.2} (reference: \"{}\")",
                    similarity, self.threshold, self.reference
                ),
            )
        }
    }
}

/// Tokenize text into lowercase word-level tokens, filtering short noise words.
fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1) // skip single-char noise
        .map(|w| w.to_lowercase())
        .collect()
}

// ─── RegexIntentMatcher ─────────────────────────────────────────────────────

/// Validates response against regex patterns representing expected behavior.
///
/// Useful when intent is best expressed as a structural pattern rather than
/// keyword list (e.g. "contains a number followed by a unit").
#[derive(Debug, Clone)]
pub struct RegexIntentMatcher {
    matcher_name: String,
    pattern: regex::Regex,
    description: String,
}

impl RegexIntentMatcher {
    /// Create a named regex intent matcher.
    pub fn new(
        name: impl Into<String>,
        pattern: &str,
        description: impl Into<String>,
    ) -> Result<Self, SemanticAssertionError> {
        let regex = regex::Regex::new(pattern)
            .map_err(|e| SemanticAssertionError::InvalidPattern(e.to_string()))?;
        Ok(Self {
            matcher_name: name.into(),
            pattern: regex,
            description: description.into(),
        })
    }
}

impl SemanticMatcher for RegexIntentMatcher {
    fn name(&self) -> &str {
        &self.matcher_name
    }

    fn evaluate(&self, response: &str) -> SemanticMatchResult {
        if self.pattern.is_match(response) {
            SemanticMatchResult::pass(
                self.name(),
                1.0,
                format!("{}: pattern matched", self.description),
            )
        } else {
            SemanticMatchResult::fail(
                self.name(),
                0.0,
                format!(
                    "{}: pattern '{}' did not match",
                    self.description,
                    self.pattern.as_str()
                ),
            )
        }
    }
}

// ─── SemanticAssertionSet ───────────────────────────────────────────────────

/// Aggregated report from evaluating a set of semantic matchers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticReport {
    /// Per-matcher results.
    pub results: Vec<SemanticMatchResult>,
    /// Overall pass/fail.
    pub passed: bool,
    /// Number of matchers that passed.
    pub passed_count: usize,
    /// Total number of matchers.
    pub total_count: usize,
    /// Average confidence across all matchers.
    pub average_confidence: f64,
}

/// Bundles multiple [`SemanticMatcher`]s for evaluating a single response.
///
/// All matchers are evaluated and results are aggregated into a [`SemanticReport`].
pub struct SemanticAssertionSet {
    matchers: Vec<Box<dyn SemanticMatcher>>,
}

impl SemanticAssertionSet {
    /// Create an empty assertion set.
    pub fn new() -> Self {
        Self {
            matchers: Vec::new(),
        }
    }

    /// Add a matcher to the set.
    pub fn add(mut self, matcher: impl SemanticMatcher + 'static) -> Self {
        self.matchers.push(Box::new(matcher));
        self
    }

    /// Number of matchers in the set.
    pub fn len(&self) -> usize {
        self.matchers.len()
    }

    /// Whether the set has no matchers.
    pub fn is_empty(&self) -> bool {
        self.matchers.is_empty()
    }

    /// Evaluate all matchers against the response and produce a report.
    pub fn evaluate(&self, response: &str) -> Result<SemanticReport, SemanticAssertionError> {
        if self.matchers.is_empty() {
            return Err(SemanticAssertionError::EmptyMatcherSet);
        }

        let results: Vec<SemanticMatchResult> = self
            .matchers
            .iter()
            .map(|m| m.evaluate(response))
            .collect();

        let passed_count = results.iter().filter(|r| r.passed).count();
        let total_count = results.len();
        let average_confidence = results.iter().map(|r| r.confidence).sum::<f64>() / total_count as f64;
        let passed = passed_count == total_count;

        Ok(SemanticReport {
            results,
            passed,
            passed_count,
            total_count,
            average_confidence,
        })
    }
}

impl Default for SemanticAssertionSet {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Integration with TurnExpectation (file-backed) ─────────────────────────

/// File-loadable semantic assertion definition (YAML/TOML/JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticExpectation {
    /// All listed facts must appear (case-insensitive).
    ContainsAllFacts { facts: Vec<String> },
    /// None of the listed terms may appear.
    ExcludesContent { banned: Vec<String> },
    /// At least one intent must match via keyword indicators.
    MatchesIntent {
        intents: Vec<IntentFile>,
        #[serde(default)]
        require_all: bool,
    },
    /// Token-overlap similarity must exceed threshold.
    SimilarTo {
        reference: String,
        #[serde(default = "default_threshold")]
        threshold: f64,
    },
    /// Regex pattern must match.
    MatchesPattern {
        name: String,
        pattern: String,
        #[serde(default)]
        description: String,
    },
}

fn default_threshold() -> f64 {
    0.5
}

/// Intent definition for file-based loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentFile {
    pub name: String,
    pub indicators: Vec<String>,
}

impl SemanticExpectation {
    /// Convert a file-backed expectation into a boxed `SemanticMatcher`.
    pub fn into_matcher(self) -> Result<Box<dyn SemanticMatcher>, SemanticAssertionError> {
        match self {
            Self::ContainsAllFacts { facts } => {
                Ok(Box::new(ContainsAllFactsMatcher::new(facts)))
            }
            Self::ExcludesContent { banned } => {
                Ok(Box::new(ExcludesContentMatcher::new(banned)))
            }
            Self::MatchesIntent {
                intents,
                require_all,
            } => {
                let mut matcher = IntentMatcher::new();
                if require_all {
                    matcher = matcher.require_all();
                }
                for intent in intents {
                    matcher = matcher.expect_intent(intent.name, intent.indicators);
                }
                Ok(Box::new(matcher))
            }
            Self::SimilarTo {
                reference,
                threshold,
            } => {
                let matcher = SimilarityMatcher::new(reference, threshold)?;
                Ok(Box::new(matcher))
            }
            Self::MatchesPattern {
                name,
                pattern,
                description,
            } => {
                let desc = if description.is_empty() {
                    name.clone()
                } else {
                    description
                };
                let matcher = RegexIntentMatcher::new(name, &pattern, desc)?;
                Ok(Box::new(matcher))
            }
        }
    }

    /// Load a list of semantic expectations from YAML.
    pub fn from_yaml_str(input: &str) -> Result<Vec<Self>, SemanticAssertionError> {
        serde_yaml::from_str(input)
            .map_err(|e| SemanticAssertionError::InvalidPattern(e.to_string()))
    }

    /// Load a list of semantic expectations from JSON.
    pub fn from_json_str(input: &str) -> Result<Vec<Self>, SemanticAssertionError> {
        serde_json::from_str(input)
            .map_err(|e| SemanticAssertionError::InvalidPattern(e.to_string()))
    }

    /// Build a `SemanticAssertionSet` from a list of expectations.
    pub fn into_assertion_set(
        expectations: Vec<Self>,
    ) -> Result<SemanticAssertionSet, SemanticAssertionError> {
        let mut set = SemanticAssertionSet::new();
        for exp in expectations {
            let matcher = exp.into_matcher()?;
            set.matchers.push(matcher);
        }
        Ok(set)
    }
}
