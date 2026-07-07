//! Runtime rule adjustment manager
//!
//! This module provides the functionality to dynamically adjust plugin rules
//! based on event priority and impact scope.

use super::event::{Event, EventPriority, ImpactScope};
use super::plugin::EventResponsePlugin;
use mofa_kernel::plugin::PluginResult;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Rule adjustment strategy
#[derive(Clone)]
pub enum RuleAdjustmentStrategy {
    /// Adjust based on event priority
    PriorityBased,
    /// Adjust based on impact scope
    ImpactScopeBased,
    /// Combined strategy
    Combined,
}

/// Rule manager
pub struct RuleManager {
    /// Plugins that can have rules adjusted
    plugins: Arc<RwLock<HashMap<String, Box<dyn EventResponsePlugin + Send + Sync>>>>,
    /// Current adjustment strategy
    strategy: RwLock<RuleAdjustmentStrategy>,
    /// Default rules configuration
    default_rules: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl Default for RuleManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleManager {
    /// Create a new rule manager
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            strategy: RwLock::new(RuleAdjustmentStrategy::Combined),
            default_rules: HashMap::new(),
        }
    }

    /// Add a plugin to the rule manager
    pub async fn add_plugin(&self, plugin: Box<dyn EventResponsePlugin + Send + Sync>) {
        let plugin_id = plugin.metadata().id.to_string();
        let mut plugins = self.plugins.write().await;
        plugins.insert(plugin_id, plugin);
    }

    /// Set the adjustment strategy
    pub async fn set_strategy(&self, strategy: RuleAdjustmentStrategy) {
        let mut current_strategy = self.strategy.write().await;
        *current_strategy = strategy;
    }

    /// Adjust rules for plugins based on the event
    pub async fn adjust_rules(&self, _event: &Event) -> PluginResult<()> {
        // Simplified implementation until we fix the plugin ownership issue
        Ok(())
    }

    /// Adjust rules based on event priority
    async fn adjust_rules_by_priority(
        &self,
        plugin: &dyn EventResponsePlugin,
        event: &Event,
    ) -> PluginResult<()> {
        // Example priority-based adjustment logic
        // For higher priority events, we might:
        // 1. Increase plugin priority
        // 2. Shorten timeout thresholds
        // 3. Increase retry counts
        // 4. Skip non-critical steps

        // Get a mutable reference to the plugin
        // Note: In real implementation, we'd need interior mutability
        info!(
            plugin_name = %plugin.metadata().name,
            priority = ?event.priority,
            "Adjusting rules for plugin based on priority"
        );

        // Example: For emergency events, skip validation steps
        if event.priority == EventPriority::Emergency {
            warn!(
                plugin_name = %plugin.metadata().name,
                "Emergency event: skipping non-critical validation steps"
            );
        }

        Ok(())
    }

    /// Adjust rules based on impact scope
    async fn adjust_rules_by_scope(
        &self,
        plugin: &dyn EventResponsePlugin,
        event: &Event,
    ) -> PluginResult<()> {
        // Example scope-based adjustment logic
        // For broader impact scopes, we might:
        // 1. Involve more team members in notifications
        // 2. Use more aggressive mitigation strategies
        // 3. Increase logging verbosity
        // 4. Trigger failover mechanisms

        info!(
            plugin_name = %plugin.metadata().name,
            scope = ?event.scope,
            "Adjusting rules for plugin based on scope"
        );

        // Example: For system-wide events, trigger failover
        if let ImpactScope::System = event.scope {
            warn!(
                plugin_name = %plugin.metadata().name,
                "System-wide event: triggering automatic failover"
            );
        }

        Ok(())
    }

    /// Get the current strategy
    pub async fn get_strategy(&self) -> RuleAdjustmentStrategy {
        self.strategy.read().await.clone()
    }

    /// Update default rules for a plugin
    pub fn update_default_rules(
        &mut self,
        plugin_id: &str,
        rules: HashMap<String, serde_json::Value>,
    ) {
        self.default_rules.insert(plugin_id.to_string(), rules);
    }
}
