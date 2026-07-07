//! swarm coordination patterns

use serde::{Deserialize, Serialize};
use mofa_kernel::agent::types::error::GlobalError;

/// Coordination pattern for a swarm of agents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationPattern {
    #[default]
    Sequential,
    Parallel,
    Debate,
    Consensus,
    MapReduce,
    Supervision,
    Routing,
}

impl std::fmt::Display for CoordinationPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sequential => write!(f, "Sequential"),
            Self::Parallel => write!(f, "Parallel"),
            Self::Debate => write!(f, "Debate"),
            Self::Consensus => write!(f, "Consensus"),
            Self::MapReduce => write!(f, "MapReduce"),
            Self::Supervision => write!(f, "Supervision"),
            Self::Routing => write!(f, "Routing"),
        }
    }
}

impl CoordinationPattern {
    /// short description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Sequential => "Agents execute subtasks one after another in dependency order",
            Self::Parallel => "Multiple agents execute independent subtasks simultaneously",
            Self::Debate => "Agents argue opposing positions, a judge synthesizes a conclusion",
            Self::Consensus => "Agents propose and iteratively converge on agreement",
            Self::MapReduce => "Input split into chunks, processed in parallel, results aggregated",
            Self::Supervision => {
                "Supervisor monitors workers, reassigns failures, escalates to HITL"
            }
            Self::Routing => "Router dispatches task to best matching specialist agent",
        }
    }

    /// minimum agents
    pub fn min_agents(&self) -> usize {
        match self {
            Self::Sequential => 1,
            Self::Parallel => 2,
            Self::Debate => 3,
            Self::Consensus => 2,
            Self::MapReduce => 2,
            Self::Supervision => 2,
            Self::Routing => 2,
        }
    }

    /// whether a leader is required
    pub fn requires_leader(&self) -> bool {
        matches!(self, Self::Debate | Self::Supervision | Self::Routing)
    }

    /// all patterns
    pub fn all() -> Vec<Self> {
        vec![
            Self::Sequential,
            Self::Parallel,
            Self::Debate,
            Self::Consensus,
            Self::MapReduce,
            Self::Supervision,
            Self::Routing,
        ]
    }

    /// create the scheduler for this pattern
    ///
    /// # Errors
    ///
    /// Returns [`GlobalError::Other`] if the scheduler for this pattern
    /// is not yet implemented. Currently implemented patterns:
    /// - Sequential
    /// - Parallel
    ///
    /// Not yet implemented (Phase 2):
    /// - Debate
    /// - Consensus
    /// - MapReduce
    /// - Supervision
    /// - Routing
    pub fn into_scheduler(self) -> Result<Box<dyn crate::swarm::SwarmScheduler>, GlobalError> {
        match self {
            Self::Sequential => Ok(Box::new(crate::swarm::SequentialScheduler::new())),
            Self::Parallel => Ok(Box::new(crate::swarm::ParallelScheduler::new())),
            Self::Debate => Err(GlobalError::Other(
                "Debate pattern scheduler not yet implemented (Phase 2)".to_string(),
            )),
            Self::Consensus => Err(GlobalError::Other(
                "Consensus pattern scheduler not yet implemented (Phase 2)".to_string(),
            )),
            Self::MapReduce => Err(GlobalError::Other(
                "MapReduce pattern scheduler not yet implemented (Phase 2)".to_string(),
            )),
            Self::Supervision => Err(GlobalError::Other(
                "Supervision pattern scheduler not yet implemented (Phase 2)".to_string(),
            )),
            Self::Routing => Err(GlobalError::Other(
                "Routing pattern scheduler not yet implemented (Phase 2)".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_roundtrip() {
        for pattern in CoordinationPattern::all() {
            let json = serde_json::to_string(&pattern).unwrap();
            let deserialized: CoordinationPattern = serde_json::from_str(&json).unwrap();
            assert_eq!(pattern, deserialized);
        }
    }

    #[test]
    fn test_yaml_parse() {
        let yaml = "\"debate\"";
        let pattern: CoordinationPattern = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(pattern, CoordinationPattern::Debate);
    }

    #[test]
    fn test_min_agents() {
        assert_eq!(CoordinationPattern::Sequential.min_agents(), 1);
        assert_eq!(CoordinationPattern::Debate.min_agents(), 3);
        assert_eq!(CoordinationPattern::MapReduce.min_agents(), 2);
    }

    #[test]
    fn test_requires_leader() {
        assert!(!CoordinationPattern::Sequential.requires_leader());
        assert!(CoordinationPattern::Debate.requires_leader());
        assert!(CoordinationPattern::Supervision.requires_leader());
    }

    #[test]
    fn test_into_scheduler_sequential_and_parallel() {
        // These should succeed
        assert!(CoordinationPattern::Sequential.into_scheduler().is_ok());
        assert!(CoordinationPattern::Parallel.into_scheduler().is_ok());
    }

    #[test]
    fn test_into_scheduler_unimplemented_returns_error() {
        // These should return errors with descriptive messages
        let unimplemented_patterns = vec![
            CoordinationPattern::Debate,
            CoordinationPattern::Consensus,
            CoordinationPattern::MapReduce,
            CoordinationPattern::Supervision,
            CoordinationPattern::Routing,
        ];

        for pattern in unimplemented_patterns {
            let result = pattern.into_scheduler();
            assert!(result.is_err(), "Expected error for {} pattern", pattern);
            
            if let Err(err) = result {
                let err_msg = format!("{}", err);
                assert!(
                    err_msg.contains("not yet implemented") || err_msg.contains("Phase 2"),
                    "Error message should mention 'not yet implemented' or 'Phase 2', got: {}",
                    err_msg
                );
            }
        }
    }
}
