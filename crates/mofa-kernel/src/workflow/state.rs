//! Graph State Trait and Types
//!
//! Defines the state management interface for workflow graphs.
//! The GraphState trait allows custom state types to work with the workflow system.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::error::AgentResult;

use super::StateUpdate;

/// Graph state trait
///
/// Implement this trait to define custom state types for workflows.
/// The trait provides methods for applying updates and serialization.
///
/// # Example
///
/// ```rust,ignore
/// use serde::{Serialize, Deserialize};
/// use mofa_kernel::workflow::GraphState;
///
/// #[derive(Clone, Serialize, Deserialize)]
/// struct MyState {
///     messages: Vec<String>,
///     result: Option<String>,
/// }
///
/// impl GraphState for MyState {
///     async fn apply_update(&mut self, key: &str, value: Value) -> AgentResult<()> {
///         match key {
///             "messages" => {
///                 if let Some(msg) = value.as_str() {
///                     self.messages.push(msg.to_string());
///                 }
///             }
///             "result" => {
///                 self.result = value.as_str().map(|s| s.to_string());
///             }
///             _ => {}
///         }
///         Ok(())
///     }
///
///     fn get_value(&self, key: &str) -> Option<Value> {
///         match key {
///                  "messages" => serde_json::to_value(&self.messages)
///                     .map_err(|e| AgentError::SerializationError(e.to_string()))
///                     .ok(),
///                     "result" => serde_json::to_value(&self.result)
///                     .map_err(|e| AgentError::SerializationError(e.to_string()))
///                    .ok(),
///
///             _ => None,
///         }
///     }
///
///     fn keys(&self) -> Vec<&str> {
///         vec!["messages", "result"]
///     }
/// }
/// ```
#[async_trait]
pub trait GraphState: Clone + Send + Sync + 'static {
    /// Apply a state update
    ///
    /// This method is called when a node returns state updates.
    /// The implementation should merge the update into the state.
    async fn apply_update<V: serde::Serialize + Send + Sync + 'static>(
        &mut self,
        key: &str,
        value: V,
    ) -> AgentResult<()>;

    /// Apply multiple updates
    async fn apply_updates<V: serde::Serialize + Send + Sync + 'static + Clone>(
        &mut self,
        updates: &[StateUpdate<V>],
    ) -> AgentResult<()> {
        for update in updates {
            self.apply_update(&update.key, update.value.clone()).await?;
        }
        Ok(())
    }

    /// Get a value by key
    ///
    /// Returns the current value for a given key, or None if the key doesn't exist.
    fn get_value<V: serde::de::DeserializeOwned + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Option<V>;

    /// Get all keys in this state
    fn keys(&self) -> Vec<&str>;

    /// Check if a key exists
    fn has_key(&self, key: &str) -> bool {
        self.keys().contains(&key)
    }

    /// Convert entire state to a JSON Value
    fn to_json(&self) -> AgentResult<Value>;

    /// Create state from a JSON Value
    fn from_json(value: Value) -> AgentResult<Self>;
}

/// State schema for validation and documentation
///
/// Describes the structure of a graph's state, including
/// key names, types, and reducer configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSchema {
    /// Schema name
    pub name: String,
    /// Field definitions
    pub fields: Vec<StateField>,
    /// Schema version
    pub version: String,
}

impl StateSchema {
    /// Create a new state schema
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
            version: "1.0".to_string(),
        }
    }

    /// Add a field to the schema
    pub fn add_field(mut self, field: StateField) -> Self {
        self.fields.push(field);
        self
    }

    /// Add a simple field
    pub fn field(mut self, name: impl Into<String>, type_name: impl Into<String>) -> Self {
        self.fields.push(StateField {
            name: name.into(),
            type_name: type_name.into(),
            description: None,
            default: None,
            required: false,
        });
        self
    }

    /// Get a field by name
    pub fn get_field(&self, name: &str) -> Option<&StateField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Get all field names
    pub fn field_names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }
}

/// A single field in the state schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateField {
    /// Field name
    pub name: String,
    /// Type name (e.g., "string", "number", "array", "object")
    pub type_name: String,
    /// Field description
    pub description: Option<String>,
    /// Default value
    pub default: Option<Value>,
    /// Whether this field is required
    pub required: bool,
}

impl StateField {
    /// Create a new state field
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            description: None,
            default: None,
            required: false,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set default value
    pub fn with_default(mut self, default: Value) -> Self {
        self.default = Some(default);
        self
    }

    /// Set required flag
    pub fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

/// A simple JSON-based state implementation
///
/// This is a basic implementation of GraphState that uses a JSON object
/// as the backing store. Useful for simple workflows or testing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsonState {
    data: serde_json::Map<String, Value>,
}

impl JsonState {
    /// Create a new empty JSON state
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from a JSON object
    pub fn from_map(data: serde_json::Map<String, Value>) -> Self {
        Self { data }
    }

    /// Create from a JSON value (must be an object)
    pub fn from_value(value: Value) -> AgentResult<Self> {
        match value {
            Value::Object(map) => Ok(Self { data: map }),
            _ => Err(crate::agent::error::AgentError::InvalidInput(
                "State must be a JSON object".to_string(),
            )),
        }
    }

    /// Get a reference to the underlying map
    pub fn as_map(&self) -> &serde_json::Map<String, Value> {
        &self.data
    }

    /// Get a mutable reference to the underlying map
    pub fn as_map_mut(&mut self) -> &mut serde_json::Map<String, Value> {
        &mut self.data
    }
}

#[async_trait]
impl GraphState for JsonState {
    async fn apply_update<V: serde::Serialize + Send + Sync + 'static>(
        &mut self,
        key: &str,
        value: V,
    ) -> AgentResult<()> {
        let json_value = serde_json::to_value(value)
            .map_err(|e| crate::agent::error::AgentError::SerializationError(e.to_string()))?;
        self.data.insert(key.to_string(), json_value);
        Ok(())
    }

    fn get_value<V: serde::de::DeserializeOwned + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Option<V> {
        self.data.get(key).and_then(|v| {
            match serde_json::from_value(v.clone()) {
                Ok(val) => Some(val),
                Err(e) => {
                    tracing::warn!(key = key, error = %e, "GraphState::get_value deserialization failed — stored type may not match requested type");
                    None
                }
            }
        })
    }

    fn keys(&self) -> Vec<&str> {
        self.data.keys().map(|s| s.as_str()).collect()
    }

    fn to_json(&self) -> AgentResult<Value> {
        Ok(Value::Object(self.data.clone()))
    }

    fn from_json(value: Value) -> AgentResult<Self> {
        Self::from_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_json_state() {
        let mut state = JsonState::new();

        state.apply_update("name", json!("test")).await.unwrap();
        state.apply_update("count", json!(42)).await.unwrap();

        assert_eq!(state.get_value("name"), Some(json!("test")));
        assert_eq!(state.get_value("count"), Some(json!(42)));
        assert!(state.has_key("name"));
        assert!(!state.has_key("unknown"));

        let keys: Vec<&str> = state.keys();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_state_schema() {
        let schema = StateSchema::new("MyState")
            .field("messages", "array")
            .field("result", "string")
            .add_field(
                StateField::new("count", "number")
                    .with_description("Execution count")
                    .with_default(json!(0))
                    .with_required(true),
            );

        assert_eq!(schema.name, "MyState");
        assert_eq!(schema.fields.len(), 3);
        assert!(schema.get_field("messages").is_some());
        assert!(schema.get_field("count").unwrap().required);
    }

    #[test]
    fn test_json_state_from_value() {
        let value = json!({
            "key1": "value1",
            "key2": 123
        });

        let state = JsonState::from_json(value).unwrap();
        assert_eq!(state.get_value("key1"), Some(json!("value1")));
        assert_eq!(state.get_value("key2"), Some(json!(123)));
    }

    #[test]
    fn test_json_state_invalid_input() {
        let result = JsonState::from_json(json!("not an object"));
        assert!(result.is_err());
    }
}
