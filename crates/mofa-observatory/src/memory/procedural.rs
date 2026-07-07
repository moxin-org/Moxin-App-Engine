use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A stored workflow template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    /// JSON-serialized workflow definition.
    pub template: serde_json::Value,
    pub tags: Vec<String>,
}

/// Procedural memory: stores and retrieves workflow templates as JSON.
#[derive(Default)]
pub struct ProceduralMemory {
    templates: HashMap<String, WorkflowTemplate>,
}

impl ProceduralMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(&mut self, template: WorkflowTemplate) {
        self.templates.insert(template.id.clone(), template);
    }

    pub fn get(&self, id: &str) -> Option<&WorkflowTemplate> {
        self.templates.get(id)
    }

    pub fn list(&self) -> Vec<&WorkflowTemplate> {
        self.templates.values().collect()
    }

    pub fn search_by_tag(&self, tag: &str) -> Vec<&WorkflowTemplate> {
        self.templates
            .values()
            .filter(|t| t.tags.iter().any(|tg| tg == tag))
            .collect()
    }

    pub fn load_from_json(&mut self, json: &str) -> Result<()> {
        let templates: Vec<WorkflowTemplate> = serde_json::from_str(json)?;
        for t in templates {
            self.store(t);
        }
        Ok(())
    }
}
