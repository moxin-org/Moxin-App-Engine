//! Circuit Breaker Configuration
//!
//! Provides configuration options for circuit breakers including
//! per-agent and global configurations.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for a circuit breaker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Name/identifier for this circuit breaker
    pub name: String,
    /// Number of consecutive failures before opening the circuit
    pub failure_threshold: u32,
    /// Number of consecutive successes needed to close the circuit from half-open
    pub success_threshold: u32,
    /// Time duration before attempting to close the circuit from open to half-open
    pub timeout: Duration,
    /// Whether the circuit breaker is enabled
    pub enabled: bool,
    /// Half-open max requests - maximum number of requests allowed in half-open state
    pub half_open_max_requests: u32,
    /// Window duration for failure rate calculation
    pub window_duration: Duration,
    /// Minimum number of requests in window before failure rate is calculated
    pub minimum_requests: u32,
    /// Failure rate threshold percentage (0-100) to open the circuit
    pub failure_rate_threshold: u32,
    /// Whether to count timeouts as failures
    pub count_timeouts_as_failures: bool,
    /// Whether to use the failure rate based opening (vs simple consecutive failures)
    pub use_failure_rate: bool,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            failure_threshold: 5,
            success_threshold: 3,
            timeout: Duration::from_secs(30),
            enabled: true,
            half_open_max_requests: 3,
            window_duration: Duration::from_secs(120),
            minimum_requests: 10,
            failure_rate_threshold: 50,
            count_timeouts_as_failures: true,
            use_failure_rate: false,
        }
    }
}

impl CircuitBreakerConfig {
    /// Create a new configuration with a name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the failure threshold
    pub fn with_failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }

    /// Set the success threshold
    pub fn with_success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold;
        self
    }

    /// Set the timeout duration
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Enable or disable the circuit breaker
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the half-open max requests
    pub fn with_half_open_max_requests(mut self, max: u32) -> Self {
        self.half_open_max_requests = max;
        self
    }

    /// Use failure rate based circuit opening
    pub fn with_failure_rate_threshold(mut self, threshold: u32) -> Self {
        self.use_failure_rate = true;
        self.failure_rate_threshold = threshold;
        self
    }

    /// Set the window duration for failure rate calculation
    pub fn with_window_duration(mut self, duration: Duration) -> Self {
        self.window_duration = duration;
        self
    }

    /// Set minimum requests for failure rate calculation
    pub fn with_minimum_requests(mut self, min: u32) -> Self {
        self.minimum_requests = min;
        self
    }

    /// Create a strict configuration (opens quickly)
    pub fn strict() -> Self {
        Self {
            failure_threshold: 3,
            success_threshold: 2,
            timeout: Duration::from_secs(10),
            enabled: true,
            half_open_max_requests: 1,
            use_failure_rate: true,
            failure_rate_threshold: 30,
            ..Default::default()
        }
    }

    /// Create a lenient configuration (requires many failures)
    pub fn lenient() -> Self {
        Self {
            failure_threshold: 10,
            success_threshold: 5,
            timeout: Duration::from_secs(60),
            enabled: true,
            half_open_max_requests: 5,
            use_failure_rate: true,
            failure_rate_threshold: 70,
            ..Default::default()
        }
    }

    /// Create a disabled configuration (no circuit breaking)
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

/// Per-agent circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCircuitBreakerConfig {
    /// Agent identifier
    pub agent_id: String,
    /// The circuit breaker configuration for this agent
    pub config: CircuitBreakerConfig,
    /// Whether to use the global config as fallback
    pub use_global_fallback: bool,
    /// Custom retry policy for this agent (uses LLMRetryPolicy from llm module)
    pub retry_config: Option<crate::llm::types::LLMRetryPolicy>,
}

impl AgentCircuitBreakerConfig {
    /// Create a new agent configuration
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            config: CircuitBreakerConfig::default(),
            use_global_fallback: true,
            retry_config: None,
        }
    }

    /// Create with custom circuit breaker config
    pub fn with_config(mut self, config: CircuitBreakerConfig) -> Self {
        self.config = config;
        self
    }

    /// Disable global fallback
    pub fn without_global_fallback(mut self) -> Self {
        self.use_global_fallback = false;
        self
    }

    /// Set custom retry config
    pub fn with_retry_config(mut self, retry_config: crate::llm::types::LLMRetryPolicy) -> Self {
        self.retry_config = Some(retry_config);
        self
    }
}

/// Global circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalCircuitBreakerConfig {
    /// Default circuit breaker configuration
    pub default_config: CircuitBreakerConfig,
    /// Per-agent configurations
    pub agent_configs: Vec<AgentCircuitBreakerConfig>,
    /// Whether to share metrics across agents
    pub share_metrics: bool,
    /// Default fallback strategy when circuit is open
    pub default_fallback_strategy: crate::circuit_breaker::fallback::FallbackStrategy,
}

impl Default for GlobalCircuitBreakerConfig {
    fn default() -> Self {
        Self {
            default_config: CircuitBreakerConfig::default(),
            agent_configs: Vec::new(),
            share_metrics: false,
            default_fallback_strategy:
                crate::circuit_breaker::fallback::FallbackStrategy::ReturnError(
                    "Circuit breaker is open".to_string(),
                ),
        }
    }
}

impl GlobalCircuitBreakerConfig {
    /// Create a new global configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the default configuration
    pub fn with_default_config(mut self, config: CircuitBreakerConfig) -> Self {
        self.default_config = config;
        self
    }

    /// Add an agent configuration
    pub fn add_agent_config(mut self, agent_config: AgentCircuitBreakerConfig) -> Self {
        self.agent_configs.push(agent_config);
        self
    }

    /// Enable metric sharing
    pub fn with_shared_metrics(mut self) -> Self {
        self.share_metrics = true;
        self
    }

    /// Set default fallback strategy
    pub fn with_default_fallback_strategy(
        mut self,
        strategy: crate::circuit_breaker::fallback::FallbackStrategy,
    ) -> Self {
        self.default_fallback_strategy = strategy;
        self
    }

    /// Get configuration for a specific agent
    pub fn get_agent_config(&self, agent_id: &str) -> Option<&AgentCircuitBreakerConfig> {
        self.agent_configs.iter().find(|c| c.agent_id == agent_id)
    }

    /// Get the effective configuration for an agent (agent config or default)
    pub fn get_effective_config(&self, agent_id: &str) -> CircuitBreakerConfig {
        self.get_agent_config(agent_id)
            .map(|c| c.config.clone())
            .unwrap_or_else(|| self.default_config.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CircuitBreakerConfig::default();
        assert!(config.enabled);
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 3);
    }

    #[test]
    fn test_strict_config() {
        let config = CircuitBreakerConfig::strict();
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.timeout.as_secs(), 10);
    }

    #[test]
    fn test_lenient_config() {
        let config = CircuitBreakerConfig::lenient();
        assert_eq!(config.failure_threshold, 10);
        assert!(config.use_failure_rate);
    }

    #[test]
    fn test_disabled_config() {
        let config = CircuitBreakerConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_agent_config() {
        let agent_config = AgentCircuitBreakerConfig::new("agent-1");
        assert_eq!(agent_config.agent_id, "agent-1");
        assert!(agent_config.use_global_fallback);
    }

    #[test]
    fn test_global_config() {
        let global = GlobalCircuitBreakerConfig::new()
            .with_default_config(CircuitBreakerConfig::strict())
            .add_agent_config(AgentCircuitBreakerConfig::new("agent-1"));

        assert_eq!(global.agent_configs.len(), 1);
        let effective = global.get_effective_config("unknown-agent");
        assert_eq!(effective.failure_threshold, 3); // from default strict config
    }
}
