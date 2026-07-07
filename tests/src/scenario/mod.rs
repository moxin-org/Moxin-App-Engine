//! Declarative scenario runner for end-to-end mock-based agent evaluation.

mod runner;
mod spec;

pub use runner::{ScenarioContext, ScenarioRunner};
pub use spec::{
    BusExpectation, BusScenarioSpec, ClockScenarioSpec, LlmFailureSpec, LlmResponseRule,
    LlmResponseSequence, LlmScenarioSpec, PromptCountExpectation, ScenarioExpectations,
    ScenarioSpec, ToolCallExpectation, ToolFailureSpec, ToolInputFailureSpec, ToolResultSpec,
    ToolScenarioSpec,
};
