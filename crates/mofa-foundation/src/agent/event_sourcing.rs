use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    Thought(ThoughtEvent),
    Action(ActionEvent),
    Observation(ObservationEvent),
    Decision(DecisionEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Message(MessageEvent),
    StateChange(StateChangeEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub thought: String,
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub action_type: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub observation: String,
    pub source: ObservationSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObservationSource {
    Tool,
    Agent,
    User,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub decision: String,
    pub confidence: Option<f64>,
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub tool_name: String,
    pub result: serde_json::Value,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChangeEvent {
    pub event_id: String,
    pub timestamp: u64,
    pub agent_id: String,
    pub state_key: String,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
}

impl AgentEvent {
    pub fn new_thought(agent_id: String, thought: String, reasoning: Option<String>) -> Self {
        Self::Thought(ThoughtEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            thought,
            reasoning,
        })
    }

    pub fn new_action(
        agent_id: String,
        action_type: String,
        parameters: serde_json::Value,
    ) -> Self {
        Self::Action(ActionEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            action_type,
            parameters,
        })
    }

    pub fn new_observation(
        agent_id: String,
        observation: String,
        source: ObservationSource,
    ) -> Self {
        Self::Observation(ObservationEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            observation,
            source,
        })
    }

    pub fn new_decision(
        agent_id: String,
        decision: String,
        confidence: Option<f64>,
        alternatives: Vec<String>,
    ) -> Self {
        Self::Decision(DecisionEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            decision,
            confidence,
            alternatives,
        })
    }

    pub fn new_tool_call(
        agent_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    ) -> Self {
        Self::ToolCall(ToolCallEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            tool_name,
            arguments,
        })
    }

    pub fn new_tool_result(
        agent_id: String,
        tool_name: String,
        result: serde_json::Value,
        success: bool,
    ) -> Self {
        Self::ToolResult(ToolResultEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            tool_name,
            result,
            success,
        })
    }

    pub fn new_message(agent_id: String, role: MessageRole, content: String) -> Self {
        Self::Message(MessageEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            role,
            content,
        })
    }

    pub fn new_state_change(
        agent_id: String,
        state_key: String,
        old_value: Option<serde_json::Value>,
        new_value: Option<serde_json::Value>,
    ) -> Self {
        Self::StateChange(StateChangeEvent {
            event_id: uuid::Uuid::now_v7().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            agent_id,
            state_key,
            old_value,
            new_value,
        })
    }

    pub fn timestamp(&self) -> u64 {
        match self {
            AgentEvent::Thought(e) => e.timestamp,
            AgentEvent::Action(e) => e.timestamp,
            AgentEvent::Observation(e) => e.timestamp,
            AgentEvent::Decision(e) => e.timestamp,
            AgentEvent::ToolCall(e) => e.timestamp,
            AgentEvent::ToolResult(e) => e.timestamp,
            AgentEvent::Message(e) => e.timestamp,
            AgentEvent::StateChange(e) => e.timestamp,
        }
    }

    pub fn agent_id(&self) -> &str {
        match self {
            AgentEvent::Thought(e) => &e.agent_id,
            AgentEvent::Action(e) => &e.agent_id,
            AgentEvent::Observation(e) => &e.agent_id,
            AgentEvent::Decision(e) => &e.agent_id,
            AgentEvent::ToolCall(e) => &e.agent_id,
            AgentEvent::ToolResult(e) => &e.agent_id,
            AgentEvent::Message(e) => &e.agent_id,
            AgentEvent::StateChange(e) => &e.agent_id,
        }
    }

    pub fn event_id(&self) -> &str {
        match self {
            AgentEvent::Thought(e) => &e.event_id,
            AgentEvent::Action(e) => &e.event_id,
            AgentEvent::Observation(e) => &e.event_id,
            AgentEvent::Decision(e) => &e.event_id,
            AgentEvent::ToolCall(e) => &e.event_id,
            AgentEvent::ToolResult(e) => &e.event_id,
            AgentEvent::Message(e) => &e.event_id,
            AgentEvent::StateChange(e) => &e.event_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSnapshot {
    pub snapshot_id: String,
    pub agent_id: String,
    pub version: u64,
    pub state: serde_json::Value,
    pub timestamp: u64,
}

pub trait EventStore: Send + Sync {
    fn append_event(&self, agent_id: &str, event: AgentEvent) -> Result<(), EventStoreError>;
    fn get_events(&self, agent_id: &str) -> Result<Vec<AgentEvent>, EventStoreError>;
    fn get_events_from(
        &self,
        agent_id: &str,
        from_version: u64,
    ) -> Result<Vec<AgentEvent>, EventStoreError>;
    fn create_snapshot(&self, snapshot: EventSnapshot) -> Result<(), EventStoreError>;
    fn get_snapshot(
        &self,
        agent_id: &str,
        version: u64,
    ) -> Result<Option<EventSnapshot>, EventStoreError>;
    fn get_latest_snapshot(&self, agent_id: &str)
    -> Result<Option<EventSnapshot>, EventStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("Event not found: {0}")]
    NotFound(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub type EventResult<T> = Result<T, EventStoreError>;

pub struct EventSourcingAgent<S: EventStore> {
    agent_id: String,
    store: S,
    version: u64,
    current_snapshot: Option<EventSnapshot>,
}

impl<S: EventStore> EventSourcingAgent<S> {
    pub fn new(agent_id: String, store: S) -> Self {
        Self {
            agent_id,
            store,
            version: 0,
            current_snapshot: None,
        }
    }

    pub async fn load_state(&mut self) -> EventResult<serde_json::Value> {
        if let Some(snapshot) = self.store.get_latest_snapshot(&self.agent_id)? {
            self.version = snapshot.version;
            self.current_snapshot = Some(snapshot.clone());
            return Ok(snapshot.state);
        }

        let events = self.store.get_events(&self.agent_id)?;
        if events.is_empty() {
            return Ok(serde_json::json!({}));
        }

        let mut state = serde_json::Value::Object(serde_json::Map::new());
        for event in events {
            state = self.apply_event(state, &event);
            self.version += 1;
        }

        Ok(state)
    }

    pub fn record_event(&self, event: AgentEvent) -> EventResult<()> {
        self.store.append_event(&self.agent_id, event)?;
        Ok(())
    }

    pub fn create_snapshot(&self, state: serde_json::Value) -> EventResult<()> {
        let snapshot = EventSnapshot {
            snapshot_id: uuid::Uuid::now_v7().to_string(),
            agent_id: self.agent_id.clone(),
            version: self.version,
            state,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.store.create_snapshot(snapshot)
    }

    /// Compact events by creating a snapshot of current state.
    /// Loads all events, applies them to reconstruct state, then creates a snapshot.
    /// Note: events before the snapshot are not removed, as this would require
    /// a mutable operation that EventStore does not support. The snapshot is used
    /// as the starting point for future state reconstruction, making prior events
    /// effectively redundant for state recovery (though preserved for audit purposes).
    pub async fn compact(&mut self) -> EventResult<()> {
        // Reconstruct current state from all events (independent of load_state
        // so we don't reset self.version from a stale snapshot)
        let events = self.store.get_events(&self.agent_id)?;
        let mut state = serde_json::json!({});
        for event in &events {
            state = self.apply_event(state, event);
        }

        // Create a snapshot at the current version
        let snapshot = EventSnapshot {
            snapshot_id: uuid::Uuid::now_v7().to_string(),
            agent_id: self.agent_id.clone(),
            version: self.version,
            state,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.store.create_snapshot(snapshot)
    }

    /// Apply a single event to agent state.
    ///
    /// Currently, only `Thought` events (which update `last_thought`) and
    /// `StateChange` events (which update keyed state) are applied to state.
    /// Other event types (Action, Observation, Decision, ToolCall, ToolResult, Message)
    /// are captured in the event log for querying but do not mutate the state object.
    fn apply_event(&self, mut state: serde_json::Value, event: &AgentEvent) -> serde_json::Value {
        match event {
            AgentEvent::Thought(t) => {
                if let Some(obj) = state.as_object_mut() {
                    obj.insert(
                        "last_thought".to_string(),
                        serde_json::Value::String(t.thought.clone()),
                    );
                }
            }
            AgentEvent::StateChange(s) => {
                if let Some(obj) = state.as_object_mut() {
                    obj.insert(
                        s.state_key.clone(),
                        s.new_value.clone().unwrap_or(serde_json::Value::Null),
                    );
                }
            }
            _ => {}
        }
        state
    }
}

pub struct EventProjector<S: EventStore> {
    store: S,
}

impl<S: EventStore> EventProjector<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn project_conversation(&self, agent_id: &str) -> EventResult<Vec<MessageEvent>> {
        let events = self.store.get_events(agent_id)?;
        let messages: Vec<MessageEvent> = events
            .into_iter()
            .filter_map(|e| {
                if let AgentEvent::Message(m) = e {
                    Some(m)
                } else {
                    None
                }
            })
            .collect();
        Ok(messages)
    }

    pub fn project_tool_usage(
        &self,
        agent_id: &str,
    ) -> EventResult<Vec<(ToolCallEvent, ToolResultEvent)>> {
        let events = self.store.get_events(agent_id)?;
        let mut tool_calls = Vec::new();
        let mut pending_call: Option<ToolCallEvent> = None;

        for event in events {
            match event {
                AgentEvent::ToolCall(call) => {
                    pending_call = Some(call);
                }
                AgentEvent::ToolResult(result) => {
                    if let Some(call) = pending_call.take() {
                        tool_calls.push((call, result));
                    }
                }
                _ => {}
            }
        }

        Ok(tool_calls)
    }
}

// ============================================================================
// InMemoryEventStore
// ============================================================================

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct InMemoryEventStore {
    events: Arc<RwLock<HashMap<String, Vec<AgentEvent>>>>,
    snapshots: Arc<RwLock<HashMap<String, Vec<EventSnapshot>>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(HashMap::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for InMemoryEventStore {
    fn append_event(&self, agent_id: &str, event: AgentEvent) -> Result<(), EventStoreError> {
        let mut events = self
            .events
            .write()
            .map_err(|e| EventStoreError::StorageError(e.to_string()))?;
        events.entry(agent_id.to_string()).or_default().push(event);
        Ok(())
    }

    fn get_events(&self, agent_id: &str) -> Result<Vec<AgentEvent>, EventStoreError> {
        let events = self
            .events
            .read()
            .map_err(|e| EventStoreError::StorageError(e.to_string()))?;
        Ok(events.get(agent_id).cloned().unwrap_or_default())
    }

    fn get_events_from(
        &self,
        agent_id: &str,
        from_version: u64,
    ) -> Result<Vec<AgentEvent>, EventStoreError> {
        // from_version is treated as a skip count (index offset into the append-only event list).
        // E.g., from_version=5 returns events at positions 5, 6, 7, ... (0-indexed).
        // This aligns with EventSourcingAgent::version which equals the number of events applied.
        let events = self
            .events
            .read()
            .map_err(|e| EventStoreError::StorageError(e.to_string()))?;
        let all = events.get(agent_id).cloned().unwrap_or_default();
        let skip = from_version as usize;
        Ok(all.into_iter().skip(skip).collect())
    }

    fn create_snapshot(&self, snapshot: EventSnapshot) -> Result<(), EventStoreError> {
        let mut snapshots = self
            .snapshots
            .write()
            .map_err(|e| EventStoreError::StorageError(e.to_string()))?;
        snapshots
            .entry(snapshot.agent_id.clone())
            .or_default()
            .push(snapshot);
        Ok(())
    }

    fn get_snapshot(
        &self,
        agent_id: &str,
        version: u64,
    ) -> Result<Option<EventSnapshot>, EventStoreError> {
        let snapshots = self
            .snapshots
            .read()
            .map_err(|e| EventStoreError::StorageError(e.to_string()))?;
        Ok(snapshots
            .get(agent_id)
            .and_then(|snaps| snaps.iter().find(|s| s.version == version).cloned()))
    }

    fn get_latest_snapshot(
        &self,
        agent_id: &str,
    ) -> Result<Option<EventSnapshot>, EventStoreError> {
        let snapshots = self
            .snapshots
            .read()
            .map_err(|e| EventStoreError::StorageError(e.to_string()))?;
        Ok(snapshots
            .get(agent_id)
            .and_then(|snaps| snaps.iter().max_by_key(|s| s.version).cloned()))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_store() -> InMemoryEventStore {
        InMemoryEventStore::new()
    }

    #[test]
    fn test_record_and_retrieve_events() {
        let store = make_store();
        let agent_id = "agent-1";

        store
            .append_event(
                agent_id,
                AgentEvent::new_thought(agent_id.to_string(), "thinking".to_string(), None),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_action(agent_id.to_string(), "move".to_string(), json!({})),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_observation(
                    agent_id.to_string(),
                    "saw something".to_string(),
                    ObservationSource::Tool,
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_decision(
                    agent_id.to_string(),
                    "go left".to_string(),
                    Some(0.9),
                    vec!["go right".to_string()],
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_message(
                    agent_id.to_string(),
                    MessageRole::User,
                    "hello".to_string(),
                ),
            )
            .unwrap();

        let events = store.get_events(agent_id).unwrap();
        assert_eq!(events.len(), 5);
        // Verify order is preserved
        assert!(matches!(&events[0], AgentEvent::Thought(_)));
        assert!(matches!(&events[4], AgentEvent::Message(_)));
    }

    #[tokio::test]
    async fn test_load_state_from_events() {
        let agent_id = "agent-2";
        let store = make_store();

        store
            .append_event(
                agent_id,
                AgentEvent::new_state_change(
                    agent_id.to_string(),
                    "name".to_string(),
                    None,
                    Some(json!("Alice")),
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_state_change(
                    agent_id.to_string(),
                    "score".to_string(),
                    None,
                    Some(json!(42)),
                ),
            )
            .unwrap();

        let mut agent = EventSourcingAgent::new(agent_id.to_string(), store);
        let state = agent.load_state().await.unwrap();

        assert_eq!(state["name"], json!("Alice"));
        assert_eq!(state["score"], json!(42));
    }

    #[tokio::test]
    async fn test_load_state_from_snapshot() {
        let agent_id = "agent-3";
        let store = make_store();

        // Create initial state via events
        store
            .append_event(
                agent_id,
                AgentEvent::new_state_change(
                    agent_id.to_string(),
                    "key".to_string(),
                    None,
                    Some(json!("initial")),
                ),
            )
            .unwrap();

        // Create snapshot
        let snapshot = EventSnapshot {
            snapshot_id: uuid::Uuid::now_v7().to_string(),
            agent_id: agent_id.to_string(),
            version: 1,
            state: json!({"key": "from_snapshot"}),
            timestamp: 12345,
        };
        store.create_snapshot(snapshot).unwrap();

        // Add more events after snapshot
        store
            .append_event(
                agent_id,
                AgentEvent::new_thought(
                    agent_id.to_string(),
                    "post-snapshot thought".to_string(),
                    None,
                ),
            )
            .unwrap();

        let mut agent = EventSourcingAgent::new(agent_id.to_string(), store);
        let state = agent.load_state().await.unwrap();

        // Should use snapshot state (not recompute from events)
        assert_eq!(state["key"], json!("from_snapshot"));
    }

    #[test]
    fn test_get_events_from_version() {
        let store = make_store();
        let agent_id = "agent-4";

        for i in 0..10 {
            store
                .append_event(
                    agent_id,
                    AgentEvent::new_thought(agent_id.to_string(), format!("thought {}", i), None),
                )
                .unwrap();
        }

        let events = store.get_events_from(agent_id, 5).unwrap();
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn test_project_conversation() {
        let agent_id = "agent-5";
        let store = make_store();

        store
            .append_event(
                agent_id,
                AgentEvent::new_message(
                    agent_id.to_string(),
                    MessageRole::User,
                    "hello".to_string(),
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_message(
                    agent_id.to_string(),
                    MessageRole::Assistant,
                    "hi there".to_string(),
                ),
            )
            .unwrap();

        let projector = EventProjector::new(store);
        let messages = projector.project_conversation(agent_id).unwrap();
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].role, MessageRole::User));
        assert!(matches!(messages[1].role, MessageRole::Assistant));
    }

    #[test]
    fn test_project_tool_usage() {
        let agent_id = "agent-6";
        let store = make_store();

        store
            .append_event(
                agent_id,
                AgentEvent::new_tool_call(
                    agent_id.to_string(),
                    "search".to_string(),
                    json!({"query": "rust"}),
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_tool_result(
                    agent_id.to_string(),
                    "search".to_string(),
                    json!(["result1"]),
                    true,
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_thought(agent_id.to_string(), "found results".to_string(), None),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_tool_call(
                    agent_id.to_string(),
                    "read_file".to_string(),
                    json!({"path": "/tmp/x"}),
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_tool_result(
                    agent_id.to_string(),
                    "read_file".to_string(),
                    json!("file content"),
                    true,
                ),
            )
            .unwrap();

        let projector = EventProjector::new(store);
        let pairs = projector.project_tool_usage(agent_id).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0.tool_name, "search");
        assert_eq!(pairs[1].0.tool_name, "read_file");
    }

    #[test]
    fn test_all_event_constructors() {
        let agent_id = "agent-7".to_string();

        let events = vec![
            AgentEvent::new_thought(agent_id.clone(), "think".to_string(), None),
            AgentEvent::new_action(agent_id.clone(), "act".to_string(), json!({})),
            AgentEvent::new_observation(
                agent_id.clone(),
                "obs".to_string(),
                ObservationSource::Tool,
            ),
            AgentEvent::new_decision(agent_id.clone(), "decide".to_string(), Some(0.8), vec![]),
            AgentEvent::new_tool_call(agent_id.clone(), "tool".to_string(), json!({})),
            AgentEvent::new_tool_result(agent_id.clone(), "tool".to_string(), json!(null), true),
            AgentEvent::new_message(agent_id.clone(), MessageRole::User, "msg".to_string()),
            AgentEvent::new_state_change(agent_id.clone(), "k".to_string(), None, Some(json!(1))),
        ];

        for event in &events {
            assert_eq!(event.agent_id(), "agent-7");
            assert!(event.timestamp() > 0);
            assert!(!event.event_id().is_empty());
        }
    }

    #[test]
    fn test_event_id_is_unique() {
        let agent_id = "agent-8".to_string();
        let mut ids = std::collections::HashSet::new();

        for _ in 0..100 {
            let event = AgentEvent::new_thought(agent_id.clone(), "thought".to_string(), None);
            ids.insert(event.event_id().to_string());
        }

        assert_eq!(ids.len(), 100);
    }

    #[tokio::test]
    async fn test_compact_creates_snapshot() {
        let store = InMemoryEventStore::new();
        let agent_id = "agent-compact";

        // Record some state changes
        store
            .append_event(
                agent_id,
                AgentEvent::new_state_change(
                    agent_id.to_string(),
                    "status".to_string(),
                    None,
                    Some(serde_json::json!("active")),
                ),
            )
            .unwrap();
        store
            .append_event(
                agent_id,
                AgentEvent::new_state_change(
                    agent_id.to_string(),
                    "count".to_string(),
                    None,
                    Some(serde_json::json!(5)),
                ),
            )
            .unwrap();

        let mut agent = EventSourcingAgent::new(agent_id.to_string(), store);

        // Compact should create a snapshot
        agent.compact().await.unwrap();

        // Verify snapshot was created (by loading state — should use snapshot path)
        let state = agent.load_state().await.unwrap();
        assert_eq!(state["status"], serde_json::json!("active"));
        assert_eq!(state["count"], serde_json::json!(5));
    }
}
