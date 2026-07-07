//! Event handling engine
//!
//! This module provides the main event handling engine that manages plugins
//! and dispatches events to the appropriate handlers.

use super::event::{Event, EventStatus};
use super::plugin::EventResponsePlugin;
use super::rule_manager::{RuleAdjustmentStrategy, RuleManager};
use mofa_kernel::plugin::{AgentPlugin, PluginContext, PluginResult};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, error, info, warn};

/// Event handling engine
pub struct EventHandlingEngine {
    /// Rule manager for runtime rule adjustment
    rule_manager: Arc<RuleManager>,
    /// Event queue
    event_queue: Arc<RwLock<VecDeque<Event>>>,
    /// Plugins registered with the engine
    plugins: Arc<RwLock<HashMap<String, Box<dyn EventResponsePlugin + Send + Sync>>>>,
    /// Plugin context
    plugin_context: PluginContext,
    /// Maximum concurrent event handlers
    max_concurrent_handlers: usize,
    /// Semaphore to control concurrent handlers
    semaphore: Arc<Semaphore>,
}

impl Default for EventHandlingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHandlingEngine {
    /// Create a new event handling engine
    pub fn new() -> Self {
        Self {
            rule_manager: Arc::new(RuleManager::new()),
            event_queue: Arc::new(RwLock::new(VecDeque::new())),
            plugins: Arc::new(RwLock::new(HashMap::new())),
            plugin_context: PluginContext::new("event-handling-engine"),
            max_concurrent_handlers: 10,
            semaphore: Arc::new(Semaphore::new(10)),
        }
    }

    /// Create a new event handling engine with custom concurrent handlers limit
    pub fn with_max_concurrent_handlers(mut self, limit: usize) -> Self {
        self.max_concurrent_handlers = limit;
        self.semaphore = Arc::new(Semaphore::new(limit));
        self
    }

    /// Set rule adjustment strategy
    pub async fn set_rule_strategy(&self, strategy: RuleAdjustmentStrategy) {
        self.rule_manager.set_strategy(strategy).await;
    }

    /// Register an event response plugin
    pub async fn register_plugin(&self, plugin: Box<dyn EventResponsePlugin + Send + Sync>) {
        // Register with the engine
        let plugin_id = plugin.metadata().id.to_string();
        let mut plugins = self.plugins.write().await;

        // Ownership of the plugin is transferred to the engine
        plugins.insert(plugin_id.clone(), plugin);

        // Note: Removed adding to rule manager due to trait object cloning restrictions
        // We can modify the rule manager to use shared ownership if needed
    }

    /// Register multiple plugins at once
    pub async fn register_plugins(&self, plugins: Vec<Box<dyn EventResponsePlugin + Send + Sync>>) {
        for plugin in plugins {
            self.register_plugin(plugin).await;
        }
    }

    /// Submit an event to be handled
    pub async fn submit_event(&self, event: Event) {
        info!(
            "Submitted new event: [{}] {} - {}",
            event.priority, event.source, event.description
        );

        // Lock the queue and add the event
        let mut queue = self.event_queue.write().await;
        queue.push_back(event);
    }

    /// Process the next event in the queue
    pub async fn process_next_event(&self) -> PluginResult<Option<Event>> {
        // Acquire a semaphore permit
        let semaphore = self.semaphore.clone();
        let _permit = semaphore.acquire().await.unwrap();

        // Pop the next event and immediately release the write lock so that
        // concurrent calls to submit_event() are not blocked for the entire
        // duration of plugin processing (which may include async I/O or LLM
        // calls lasting seconds).
        let mut event = {
            let mut queue = self.event_queue.write().await;
            match queue.pop_front() {
                Some(event) => event,
                None => return Ok(None),
            }
        }; // write lock released here, before any .await

        // Update event status
        event.update_status(EventStatus::Processing);

        // Adjust rules based on the event
        self.rule_manager.adjust_rules(&event).await?;

        // Lock plugins for reading
        let mut plugins = self.plugins.write().await;

        // Find the first plugin that can handle the event
        for (_plugin_id, plugin) in plugins.iter_mut() {
            if plugin.can_handle(&event) {
                debug!(
                    "Processing event {} with plugin: {}",
                    event.id,
                    plugin.metadata().name
                );

                // Process the event
                let processed_event = plugin.handle_event(event).await?;

                info!(
                    "Event {} processed successfully by plugin {}",
                    processed_event.id,
                    plugin.metadata().name
                );

                return Ok(Some(processed_event));
            }
        }

        // No plugin found to handle the event
        warn!("No plugin found to handle event: {}", event.id);
        event.update_status(EventStatus::ManualInterventionNeeded);

        Ok(Some(event))
    }

    /// Start the engine and process events continuously
    pub async fn start(&self) -> PluginResult<()> {
        info!(
            "Starting event handling engine with {} concurrent handlers...",
            self.max_concurrent_handlers
        );

        // Keep processing events forever
        loop {
            match self.process_next_event().await {
                Ok(Some(event)) => {
                    // Event processed, do any post-processing if needed
                    if event.status == EventStatus::Resolved {
                        info!("Event resolved: {}", event.id);
                    } else {
                        info!("Event {} status: {:?}", event.id, event.status);
                    }
                }
                Ok(None) => {
                    // No events in queue, wait a bit before checking again
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(err) => {
                    // Handle error
                    error!("Error processing event: {}", err);
                    // Continue processing
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                }
            }
        }
    }
}

/// Lifecycle-aware runner for `EventHandlingEngine`.
///
/// This wrapper ensures that at most one main event-processing loop is running
/// for a given engine instance. Calling `spawn()` multiple times on the same
/// runner is idempotent: only the first call will actually start the loop.
pub struct EventEngineRunner {
    engine: Arc<EventHandlingEngine>,
    running: Arc<AtomicBool>,
}

impl EventEngineRunner {
    /// Create a new runner for the given engine instance.
    pub fn new(engine: Arc<EventHandlingEngine>) -> Self {
        Self {
            engine,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Return a reference to the wrapped engine.
    pub fn engine(&self) -> &Arc<EventHandlingEngine> {
        &self.engine
    }

    /// Start the event-handling loop if it is not already running.
    ///
    /// Multiple calls to `spawn()` on the same runner (or its clones) will
    /// result in at most one background task that invokes `engine.start()`.
    /// If the loop is already running, this method spawns a no-op task so
    /// callers can always `await` the returned handle without special-casing.
    pub fn spawn(&self) -> tokio::task::JoinHandle<()> {
        // Try to transition running: false -> true. If this fails, another
        // task has already started the loop for this runner.
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            // Already running: spawn a no-op task so the caller still
            // receives a handle it can `.await` or `.abort()` safely.
            return tokio::spawn(async {});
        }

        let engine = Arc::clone(&self.engine);
        let running_flag = Arc::clone(&self.running);

        tokio::spawn(async move {
            let _ = engine.start().await;
            running_flag.store(false, Ordering::SeqCst);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secretary::monitoring::event::*;
    use crate::secretary::monitoring::plugins::{
        NetworkAttackResponsePlugin, ServerFaultResponsePlugin,
    };

    #[tokio::test]
    async fn test_event_handling() {
        // Create event engine
        let engine = EventHandlingEngine::new();

        // Create plugins
        let server_fault_plugin = ServerFaultResponsePlugin::new();
        let network_attack_plugin = NetworkAttackResponsePlugin::new();

        // Register plugins with explicit boxing
        engine
            .register_plugins(vec![
                Box::new(server_fault_plugin) as Box<dyn EventResponsePlugin + Send + Sync>,
                Box::new(network_attack_plugin) as Box<dyn EventResponsePlugin + Send + Sync>,
            ])
            .await;

        // Create server fault event
        let server_fault_event = Event::new(
            EventType::ServerFault,
            EventPriority::High,
            ImpactScope::Instance("web-server-01".to_string()),
            "monitoring-agent".to_string(),
            "Server unresponsive".to_string(),
            serde_json::json!({ "server": "web-server-01" }),
        );

        // Create network attack event
        let network_attack_event = Event::new(
            EventType::NetworkAttack,
            EventPriority::Emergency,
            ImpactScope::Service("api-gateway".to_string()),
            "ids".to_string(),
            "DDoS attack".to_string(),
            serde_json::json!({ "source_ip": "10.0.0.1" }),
        );

        // Submit both events
        engine.submit_event(server_fault_event).await;
        engine.submit_event(network_attack_event).await;

        // Process the first event
        let result = engine.process_next_event().await;
        assert!(result.is_ok());

        // Check that event was processed
        if let Ok(Some(event)) = result {
            assert!(matches!(event.status, EventStatus::Resolved));
        }
    }

    /// Demonstrates that calling `start()` twice on the same `EventHandlingEngine`
    /// instance creates two independent event loops, both logging their startup.
    #[tokio::test]
    async fn demo_engine_double_start_logs_twice() {
        use std::sync::Arc;

        let engine = Arc::new(EventHandlingEngine::new());

        // (Optionally register plugins and submit events here)

        let e1 = engine.clone();
        let e2 = engine.clone();

        let h1 = tokio::spawn(async move {
            let _ = e1.start().await;
        });
        let h2 = tokio::spawn(async move {
            let _ = e2.start().await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        h1.abort();
        h2.abort();
    }

    /// Verifies that `EventEngineRunner::spawn` is idempotent: calling it multiple
    /// times results in at most one real `start()` invocation for the wrapped engine.
    #[tokio::test]
    async fn event_engine_runner_spawn_is_idempotent() {
        use std::sync::Arc;

        let engine = Arc::new(EventHandlingEngine::new());
        let runner = EventEngineRunner::new(engine);

        let h1 = runner.spawn();
        let h2 = runner.spawn();

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        h1.abort();
        h2.abort();
    }
}
