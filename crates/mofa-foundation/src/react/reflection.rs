//! Reflection agentic design pattern
//!
//! Implements a generate → critique → refine loop: an agent produces a draft,
//! critiques it (self or via a separate critic), then refines the draft.
//! Uses the kernel's `ThoughtStepType::Reflection` and `ThoughtStep::reflection()`
//! for reflection steps in the trace.
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::react::{ReflectionAgent, ReflectionConfig};
//! use std::sync::Arc;
//!
//! let generator = Arc::new(llm_agent);
//! let critic = Arc::new(critic_llm_agent); // optional, can use same agent
//!
//! let agent = ReflectionAgent::builder()
//!     .with_generator(generator)
//!     .with_critic(critic)
//!     .with_config(ReflectionConfig::default().with_max_rounds(3))
//!     .build();
//!
//! let result = agent.run("Explain quantum entanglement").await?;
//! println!("Final: {}", result.final_answer);
//! ```

use crate::agent::components::ThoughtStep;
use crate::llm::{LLMAgent, LLMError, LLMResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ============================================================================
// Config and types
// ============================================================================

/// Configuration for the Reflection (critique–refine) loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionConfig {
    /// Maximum number of refine rounds (generate → critique → refine).
    #[serde(default = "default_max_rounds")]
    pub max_rounds: usize,
    /// System or prefix prompt for the critic (use {task} and {draft}).
    #[serde(default)]
    pub critic_prompt_template: Option<String>,
    /// System or prefix prompt for the refine step (use {task}, {draft}, {critique}).
    #[serde(default)]
    pub refine_prompt_template: Option<String>,
    /// Whether to log steps.
    #[serde(default = "default_verbose")]
    pub verbose: bool,
}

fn default_max_rounds() -> usize {
    3
}

fn default_verbose() -> bool {
    true
}

impl Default for ReflectionConfig {
    fn default() -> Self {
        Self {
            max_rounds: default_max_rounds(),
            critic_prompt_template: None,
            refine_prompt_template: None,
            verbose: default_verbose(),
        }
    }
}

impl ReflectionConfig {
    pub fn with_max_rounds(mut self, n: usize) -> Self {
        self.max_rounds = n;
        self
    }

    pub fn with_critic_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.critic_prompt_template = Some(prompt.into());
        self
    }

    pub fn with_refine_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.refine_prompt_template = Some(prompt.into());
        self
    }

    pub fn with_verbose(mut self, v: bool) -> Self {
        self.verbose = v;
        self
    }
}

/// Default critic prompt template. Placeholders: {task}, {draft}.
const DEFAULT_CRITIC_PROMPT: &str = r#"You are a critical reviewer. Given the task and the draft below, list specific improvements (clarity, accuracy, completeness). Be concise. If the draft is already good, say "No major improvements needed."

Task: {task}

Draft:
{draft}

Critique:"#;

/// Default refine prompt template. Placeholders: {task}, {draft}, {critique}.
const DEFAULT_REFINE_PROMPT: &str = r#"You are improving a draft based on critique. Produce a revised version that addresses the feedback. Output only the improved text, no meta-commentary.

Task: {task}

Previous draft:
{draft}

Critique:
{critique}

Improved version:"#;

/// One round of the reflection loop: draft, critique, and optional refined output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionStep {
    /// Round index (0-based).
    pub round: usize,
    /// Draft produced in this round (before critique).
    pub draft: String,
    /// Critique of the draft.
    pub critique: String,
    /// Refined draft after applying critique (empty on last round if no further refine).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub refined: String,
}

/// Result of running the Reflection agent.
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionResult {
    /// Original task.
    pub task: String,
    /// Final answer (last draft or last refined).
    pub final_answer: String,
    /// Per-round steps (draft, critique, refined).
    pub steps: Vec<ReflectionStep>,
    /// Kernel-compatible reflection steps for tracing (each critique as ThoughtStep::Reflection).
    pub reflection_steps: Vec<ThoughtStep>,
    /// Number of rounds executed.
    pub rounds: usize,
    /// Whether the run completed without error.
    pub success: bool,
    /// Error message if success is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Total duration in milliseconds.
    pub duration_ms: u64,
}

// ============================================================================
// ReflectionAgent
// ============================================================================

/// Agent that runs a generate → critique → refine loop using a generator
/// and an optional critic (defaults to same agent as critic).
pub struct ReflectionAgent {
    /// Agent used to generate drafts and (optionally) refined versions.
    generator: Arc<LLMAgent>,
    /// Agent used to critique drafts. If None, generator is used as critic.
    critic: Option<Arc<LLMAgent>>,
    config: ReflectionConfig,
}

impl ReflectionAgent {
    /// Start a builder for ReflectionAgent.
    pub fn builder() -> ReflectionAgentBuilder {
        ReflectionAgentBuilder::new()
    }

    /// Run the reflection loop: generate → critique → refine (repeat up to max_rounds).
    pub async fn run(&self, task: impl Into<String>) -> LLMResult<ReflectionResult> {
        let task = task.into();
        let start = std::time::Instant::now();
        let mut steps = Vec::new();
        let mut reflection_steps = Vec::new();
        let critic_agent = self.critic.as_ref().unwrap_or(&self.generator);

        let mut draft = self.generate_draft(&task).await?;
        let mut step_number = 0;

        for round in 0..self.config.max_rounds {
            if self.config.verbose {
                tracing::info!(
                    "[Reflection] Round {} - critiquing draft (len={})",
                    round + 1,
                    draft.len()
                );
            }

            let critique = self.run_critique(critic_agent, &task, &draft).await?;
            // Add stopping condition here
            if critique.to_lowercase().contains("no major improvements")
                || critique.to_lowercase().contains("no more improvements")
            {
                if self.config.verbose {
                    tracing::info!(
                        "[Reflection] Stopping early - critic satisfied at round {}",
                        round + 1
                    );
                }
                step_number += 1;
                steps.push(ReflectionStep {
                    round,
                    draft: draft.clone(),
                    critique: critique.clone(),
                    refined: String::new(),
                });
                break;
            }
            step_number += 1;
            reflection_steps.push(ThoughtStep::reflection(critique.clone(), step_number));

            let refined = if round + 1 < self.config.max_rounds {
                if self.config.verbose {
                    tracing::info!("[Reflection] Round {} - refining", round + 1);
                }
                self.run_refine(&task, &draft, &critique).await?
            } else {
                String::new()
            };

            steps.push(ReflectionStep {
                round,
                draft: draft.clone(),
                critique: critique.clone(),
                refined: refined.clone(),
            });

            if refined.is_empty() {
                break;
            }
            draft = refined;
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        if self.config.verbose {
            tracing::info!(
                "[Reflection] Completed in {} rounds, {}ms",
                steps.len(),
                duration_ms
            );
        }

        let rounds = steps.len();
        Ok(ReflectionResult {
            task: task.clone(),
            final_answer: draft,
            steps,
            reflection_steps,
            rounds,
            success: true,
            error: None,
            duration_ms,
        })
    }

    async fn generate_draft(&self, task: &str) -> LLMResult<String> {
        self.generator.ask(task).await
    }

    async fn run_critique(&self, critic: &LLMAgent, task: &str, draft: &str) -> LLMResult<String> {
        let prompt = self
            .config
            .critic_prompt_template
            .as_deref()
            .unwrap_or(DEFAULT_CRITIC_PROMPT)
            .replace("{task}", task)
            .replace("{draft}", draft);
        critic.ask(&prompt).await
    }

    async fn run_refine(&self, task: &str, draft: &str, critique: &str) -> LLMResult<String> {
        let prompt = self
            .config
            .refine_prompt_template
            .as_deref()
            .unwrap_or(DEFAULT_REFINE_PROMPT)
            .replace("{task}", task)
            .replace("{draft}", draft)
            .replace("{critique}", critique);
        self.generator.ask(&prompt).await
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for ReflectionAgent.
pub struct ReflectionAgentBuilder {
    generator: Option<Arc<LLMAgent>>,
    critic: Option<Arc<LLMAgent>>,
    config: ReflectionConfig,
}

impl ReflectionAgentBuilder {
    pub fn new() -> Self {
        Self {
            generator: None,
            critic: None,
            config: ReflectionConfig::default(),
        }
    }

    pub fn with_generator(mut self, agent: Arc<LLMAgent>) -> Self {
        self.generator = Some(agent);
        self
    }

    pub fn with_critic(mut self, agent: Arc<LLMAgent>) -> Self {
        self.critic = Some(agent);
        self
    }

    pub fn with_config(mut self, config: ReflectionConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_max_rounds(mut self, n: usize) -> Self {
        self.config.max_rounds = n;
        self
    }

    pub fn with_verbose(mut self, v: bool) -> Self {
        self.config.verbose = v;
        self
    }

    pub fn build(self) -> Result<ReflectionAgent, LLMError> {
        let generator = self.generator.ok_or_else(|| {
            LLMError::Other("ReflectionAgent requires a generator (with_generator)".to_string())
        })?;
        Ok(ReflectionAgent {
            generator,
            critic: self.critic,
            config: self.config,
        })
    }
}

impl Default for ReflectionAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let c = ReflectionConfig::default();
        assert_eq!(c.max_rounds, 3);
        assert!(c.verbose);
    }

    #[test]
    fn config_builder() {
        let c = ReflectionConfig::default()
            .with_max_rounds(5)
            .with_verbose(false);
        assert_eq!(c.max_rounds, 5);
        assert!(!c.verbose);
    }

    #[test]
    fn reflection_step_serialization() {
        let step = ReflectionStep {
            round: 0,
            draft: "d".to_string(),
            critique: "c".to_string(),
            refined: "r".to_string(),
        };
        let j = serde_json::to_string(&step).unwrap();
        assert!(j.contains("\"round\":0"));
        assert!(j.contains("d"));
        assert!(j.contains("c"));
        assert!(j.contains("r"));
    }
}
