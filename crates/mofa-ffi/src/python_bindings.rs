//! PyO3 Python bindings for MoFA
//!
//! This module provides native Python extension bindings.

use pyo3::exceptions::{PyNotImplementedError, PyRuntimeError};
use pyo3::prelude::*;

use crate::swarm::{SwarmOrchestratorFFI, SwarmResultFFI};

// Note: Python bindings are being refactored to use MoFAAgent directly.
// The PyAgentWrapper will be reimplemented to wrap MoFAAgent instead of RuntimeAgent.

// =============================================================================
// Swarm Orchestrator — PyO3 wrappers
// =============================================================================

/// Result of a swarm execution, returned by `SwarmOrchestrator.run_goal()`.
///
/// Attributes
/// ----------
/// execution_id : str
///     UUID for this run. Correlates with OTel traces and audit JSONL.
/// goal : str
///     The original natural-language goal string.
/// tasks_succeeded : int
///     Number of subtasks that completed successfully.
/// tasks_failed : int
///     Number of subtasks that failed or were rejected by a HITL gate.
/// wall_time_ms : int
///     Total wall-clock execution time in milliseconds.
#[pyclass(name = "SwarmResult")]
#[derive(Clone)]
pub struct PySwarmResult {
    #[pyo3(get)]
    pub execution_id: String,
    #[pyo3(get)]
    pub goal: String,
    #[pyo3(get)]
    pub tasks_succeeded: u64,
    #[pyo3(get)]
    pub tasks_failed: u64,
    #[pyo3(get)]
    pub wall_time_ms: u64,
}

#[pymethods]
impl PySwarmResult {
    fn __repr__(&self) -> String {
        format!(
            "SwarmResult(execution_id={:?}, goal={:?}, succeeded={}, failed={}, wall_time_ms={})",
            self.execution_id,
            self.goal,
            self.tasks_succeeded,
            self.tasks_failed,
            self.wall_time_ms,
        )
    }
}

impl From<SwarmResultFFI> for PySwarmResult {
    fn from(r: SwarmResultFFI) -> Self {
        Self {
            execution_id: r.execution_id,
            goal: r.goal,
            tasks_succeeded: r.tasks_succeeded,
            tasks_failed: r.tasks_failed,
            wall_time_ms: r.wall_time_ms,
        }
    }
}

/// High-level entry point that connects a natural-language goal to a coordinated
/// multi-agent execution across all Cognitive Swarm Orchestrator modules.
///
/// Owns an internal Tokio runtime — no async machinery needed on the Python side.
///
/// Parameters
/// ----------
/// name : str
///     Display name for this orchestrator. Appears in OTel span attributes
///     and SwarmAuditLog entries.
///
/// Examples
/// --------
/// >>> from mofa import SwarmOrchestrator
/// >>> orch = SwarmOrchestrator("compliance-swarm")
/// >>> result = orch.run_goal("review Q1 loan applications for fair lending violations")
/// >>> print(f"succeeded={result.tasks_succeeded}  id={result.execution_id}")
#[pyclass(name = "SwarmOrchestrator")]
pub struct PySwarmOrchestrator {
    inner: SwarmOrchestratorFFI,
}

#[pymethods]
impl PySwarmOrchestrator {
    #[new]
    fn new(name: String) -> Self {
        Self {
            inner: SwarmOrchestratorFFI::new(name),
        }
    }

    /// Run the full swarm pipeline for the given natural-language goal.
    ///
    /// Blocks until execution completes or raises RuntimeError on failure.
    ///
    /// Parameters
    /// ----------
    /// goal : str
    ///     Natural-language description of the task to execute.
    ///
    /// Returns
    /// -------
    /// SwarmResult
    fn run_goal(&self, goal: String) -> PyResult<PySwarmResult> {
        self.inner
            .run_goal(goal)
            .map(PySwarmResult::from)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }
}

// =============================================================================
// Module initialization
// =============================================================================

/// Python module initialization
#[pymodule]
pub fn mofa(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_agents_py, m)?)?;
    m.add_class::<PySwarmOrchestrator>()?;
    m.add_class::<PySwarmResult>()?;
    Ok(())
}

/// Run a Python agent (placeholder)
#[pyfunction]
fn run_agents_py(_py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    Err(PyNotImplementedError::new_err(
        "mofa.run_agents_py is not implemented; use UniFFI LLMAgent APIs for now",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN_AGENTS_UNSUPPORTED_MESSAGE: &str =
        "mofa.run_agents_py is not implemented; use UniFFI LLMAgent APIs for now";

    #[test]
    fn run_agents_py_returns_explicit_unsupported_error() {
        Python::attach(|py| {
            let err = run_agents_py(py).expect_err("placeholder success must be removed");
            assert!(err.is_instance_of::<PyNotImplementedError>(py));
            assert!(err.to_string().contains(RUN_AGENTS_UNSUPPORTED_MESSAGE));
        });
    }

    #[test]
    fn py_swarm_orchestrator_new_does_not_panic() {
        let _orch = PySwarmOrchestrator::new("test".to_string());
    }

    #[test]
    fn py_swarm_result_from_ffi() {
        let ffi = SwarmResultFFI {
            execution_id: "abc-123".to_string(),
            goal: "test goal".to_string(),
            tasks_succeeded: 3,
            tasks_failed: 1,
            wall_time_ms: 250,
        };
        let py_result = PySwarmResult::from(ffi);
        assert_eq!(py_result.execution_id, "abc-123");
        assert_eq!(py_result.tasks_succeeded, 3);
    }
}
