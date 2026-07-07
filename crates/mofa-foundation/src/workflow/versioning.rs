use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowVersion {
    pub version: String,
    pub created_at: u64,
    pub author: Option<String>,
    pub changelog: String,
    pub workflow_id: String,
    pub definition: WorkflowDefinitionHash,
    pub status: VersionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinitionHash {
    pub hash: String,
    pub format: SerializationFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerializationFormat {
    Json,
    Yaml,
    Ron,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionStatus {
    Draft,
    Published,
    Archived,
    Deprecated,
}

impl WorkflowVersion {
    pub fn new(
        version: String,
        workflow_id: String,
        definition_hash: String,
        changelog: String,
    ) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            version,
            created_at,
            author: None,
            changelog,
            workflow_id,
            definition: WorkflowDefinitionHash {
                hash: definition_hash,
                format: SerializationFormat::Json,
            },
            status: VersionStatus::Draft,
        }
    }

    pub fn publish(&mut self) {
        self.status = VersionStatus::Published;
    }

    pub fn archive(&mut self) {
        self.status = VersionStatus::Archived;
    }

    pub fn deprecate(&mut self) {
        self.status = VersionStatus::Deprecated;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowHistory {
    pub workflow_id: String,
    pub versions: Vec<WorkflowVersion>,
    pub current_version: Option<String>,
}

impl WorkflowHistory {
    pub fn new(workflow_id: String) -> Self {
        Self {
            workflow_id,
            versions: Vec::new(),
            current_version: None,
        }
    }

    pub fn add_version(&mut self, version: WorkflowVersion) {
        self.versions.push(version);
    }

    pub fn get_version(&self, version: &str) -> Option<&WorkflowVersion> {
        self.versions.iter().find(|v| v.version == version)
    }

    pub fn set_current_version(&mut self, version: String) {
        self.current_version = Some(version);
    }

    pub fn rollback(&mut self, target_version: &str) -> Option<&WorkflowVersion> {
        if self.versions.iter().any(|v| v.version == target_version) {
            self.current_version = Some(target_version.to_string());
            self.get_version(target_version)
        } else {
            None
        }
    }
}

pub trait VersionStore: Send + Sync {
    fn save_version(&self, history: WorkflowHistory) -> Result<(), VersionError>;
    fn load_history(&self, workflow_id: &str) -> Result<Option<WorkflowHistory>, VersionError>;
    fn list_versions(&self, workflow_id: &str) -> Result<Vec<String>, VersionError>;
    fn delete_version(&self, workflow_id: &str, version: &str) -> Result<(), VersionError>;
}

#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    #[error("Version not found: {0}")]
    NotFound(String),

    #[error("Invalid version: {0}")]
    InvalidVersion(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Circular dependency detected")]
    CircularDependency,

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

pub type VersionResult<T> = Result<T, VersionError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDiff {
    pub from_version: String,
    pub to_version: String,
    pub changes: Vec<VersionChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionChange {
    Added {
        path: String,
        value: serde_json::Value,
    },
    Removed {
        path: String,
        value: serde_json::Value,
    },
    Modified {
        path: String,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowMigration {
    pub from_version: String,
    pub to_version: String,
    pub migration_steps: Vec<MigrationStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStep {
    pub description: String,
    pub is_breaking: bool,
}

pub struct InMemoryVersionStore {
    data: Arc<RwLock<HashMap<String, WorkflowHistory>>>,
}

impl InMemoryVersionStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryVersionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionStore for InMemoryVersionStore {
    fn save_version(&self, history: WorkflowHistory) -> Result<(), VersionError> {
        let mut data = self
            .data
            .write()
            .map_err(|e| VersionError::StorageError(e.to_string()))?;
        data.insert(history.workflow_id.clone(), history);
        Ok(())
    }

    fn load_history(&self, workflow_id: &str) -> Result<Option<WorkflowHistory>, VersionError> {
        let data = self
            .data
            .read()
            .map_err(|e| VersionError::StorageError(e.to_string()))?;
        Ok(data.get(workflow_id).cloned())
    }

    fn list_versions(&self, workflow_id: &str) -> Result<Vec<String>, VersionError> {
        let data = self
            .data
            .read()
            .map_err(|e| VersionError::StorageError(e.to_string()))?;
        Ok(data
            .get(workflow_id)
            .map(|h| h.versions.iter().map(|v| v.version.clone()).collect())
            .unwrap_or_default())
    }

    fn delete_version(&self, workflow_id: &str, version: &str) -> Result<(), VersionError> {
        let mut data = self
            .data
            .write()
            .map_err(|e| VersionError::StorageError(e.to_string()))?;
        let history = data
            .get_mut(workflow_id)
            .ok_or_else(|| VersionError::NotFound(workflow_id.to_string()))?;

        let before = history.versions.len();
        history.versions.retain(|v| v.version != version);
        if history.versions.len() == before {
            return Err(VersionError::NotFound(version.to_string()));
        }
        if history.current_version.as_deref() == Some(version) {
            history.current_version = history.versions.last().map(|v| v.version.clone());
        }
        Ok(())
    }
}

pub struct VersionManager<S: VersionStore> {
    store: S,
}

impl<S: VersionStore> VersionManager<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn create_version(
        &self,
        workflow_id: String,
        definition: &str,
        version: String,
        changelog: String,
    ) -> VersionResult<WorkflowVersion> {
        let hash = calculate_hash(definition);

        let mut workflow_version = WorkflowVersion::new(version, workflow_id, hash, changelog);
        workflow_version.publish();

        let mut history = self
            .store
            .load_history(&workflow_version.workflow_id)?
            .unwrap_or_else(|| WorkflowHistory::new(workflow_version.workflow_id.clone()));

        history.add_version(workflow_version.clone());
        history.set_current_version(workflow_version.version.clone());

        self.store.save_version(history)?;

        Ok(workflow_version)
    }

    /// Create a version in Draft status (does not publish immediately)
    pub fn create_draft(
        &self,
        workflow_id: String,
        definition: &str,
        version: String,
        changelog: String,
    ) -> VersionResult<WorkflowVersion> {
        let hash = calculate_hash(definition);
        let workflow_version = WorkflowVersion::new(version, workflow_id.clone(), hash, changelog);
        // status remains Draft

        let mut history = self
            .store
            .load_history(&workflow_id)?
            .unwrap_or_else(|| WorkflowHistory::new(workflow_id));

        history.add_version(workflow_version.clone());
        self.store.save_version(history)?;
        Ok(workflow_version)
    }

    /// Promote a draft version to Published status
    pub fn publish_version(
        &self,
        workflow_id: &str,
        version: &str,
    ) -> VersionResult<WorkflowVersion> {
        let mut history = self
            .store
            .load_history(workflow_id)?
            .ok_or_else(|| VersionError::NotFound(workflow_id.to_string()))?;

        let wv = history
            .versions
            .iter_mut()
            .find(|v| v.version == version)
            .ok_or_else(|| VersionError::NotFound(version.to_string()))?;

        wv.publish();
        let published = wv.clone();

        history.set_current_version(version.to_string());
        self.store.save_version(history)?;
        Ok(published)
    }

    pub fn rollback(
        &self,
        workflow_id: &str,
        target_version: &str,
    ) -> VersionResult<WorkflowVersion> {
        let mut history = self
            .store
            .load_history(workflow_id)?
            .ok_or_else(|| VersionError::NotFound(workflow_id.to_string()))?;

        let version = history
            .rollback(target_version)
            .ok_or_else(|| VersionError::NotFound(target_version.to_string()))?
            .clone();

        self.store.save_version(history)?;

        Ok(version)
    }

    pub fn diff(&self, workflow_id: &str, from: &str, to: &str) -> VersionResult<VersionDiff> {
        let history = self
            .store
            .load_history(workflow_id)?
            .ok_or_else(|| VersionError::NotFound(workflow_id.to_string()))?;

        let _from_version = history
            .get_version(from)
            .ok_or_else(|| VersionError::NotFound(from.to_string()))?;
        let _to_version = history
            .get_version(to)
            .ok_or_else(|| VersionError::NotFound(to.to_string()))?;

        // TODO: Full content diff requires storing definition content alongside the hash.
        // Currently only the SHA-256 hash is stored in WorkflowDefinitionHash.
        // To enable structural diffing, extend WorkflowDefinitionHash with a `content: String`
        // field and parse both as serde_json::Value to produce Added/Removed/Modified entries.
        Err(VersionError::NotImplemented(
            "content diff requires stored definitions; only hash is currently persisted"
                .to_string(),
        ))
    }
}

fn calculate_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> VersionManager<InMemoryVersionStore> {
        VersionManager::new(InMemoryVersionStore::new())
    }

    #[test]
    fn test_create_version_and_retrieve() {
        let manager = make_manager();
        let v = manager
            .create_version(
                "wf-1".to_string(),
                r#"{"steps":["a"]}"#,
                "1.0.0".to_string(),
                "initial".to_string(),
            )
            .unwrap();
        assert_eq!(v.version, "1.0.0");
        assert_eq!(v.workflow_id, "wf-1");

        let history = manager.store().load_history("wf-1").unwrap().unwrap();
        let retrieved = history.get_version("1.0.0").unwrap();
        assert_eq!(retrieved.changelog, "initial");
    }

    #[test]
    fn test_publish_and_rollback() {
        let manager = make_manager();
        manager
            .create_version(
                "wf-2".to_string(),
                r#"{"v":1}"#,
                "1.0.0".to_string(),
                "v1".to_string(),
            )
            .unwrap();
        manager
            .create_version(
                "wf-2".to_string(),
                r#"{"v":2}"#,
                "2.0.0".to_string(),
                "v2".to_string(),
            )
            .unwrap();

        manager.rollback("wf-2", "1.0.0").unwrap();
        let history = manager.store().load_history("wf-2").unwrap().unwrap();
        assert_eq!(history.current_version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn test_rollback_nonexistent_version_returns_error() {
        let manager = make_manager();
        manager
            .create_version(
                "wf-3".to_string(),
                r#"{}"#,
                "1.0.0".to_string(),
                "init".to_string(),
            )
            .unwrap();
        let err = manager.rollback("wf-3", "v999").unwrap_err();
        assert!(matches!(err, VersionError::NotFound(_)));
    }

    #[test]
    fn test_version_status_lifecycle() {
        let mut v = WorkflowVersion::new(
            "1.0.0".to_string(),
            "wf-4".to_string(),
            "hash".to_string(),
            "init".to_string(),
        );
        assert!(matches!(v.status, VersionStatus::Draft));
        v.publish();
        assert!(matches!(v.status, VersionStatus::Published));
        v.archive();
        assert!(matches!(v.status, VersionStatus::Archived));
        v.deprecate();
        assert!(matches!(v.status, VersionStatus::Deprecated));
    }

    #[test]
    fn test_list_versions() {
        let store = InMemoryVersionStore::new();
        let manager = VersionManager::new(store);
        for i in 1..=3 {
            manager
                .create_version(
                    "wf-5".to_string(),
                    &format!(r#"{{"v":{}}}"#, i),
                    format!("{}.0.0", i),
                    format!("v{}", i),
                )
                .unwrap();
        }
        let versions = manager.store().list_versions("wf-5").unwrap();
        assert_eq!(versions.len(), 3);
    }

    #[test]
    fn test_delete_version() {
        let store = InMemoryVersionStore::new();
        let manager = VersionManager::new(store);
        manager
            .create_version(
                "wf-6".to_string(),
                r#"{"v":1}"#,
                "1.0.0".to_string(),
                "v1".to_string(),
            )
            .unwrap();
        manager
            .create_version(
                "wf-6".to_string(),
                r#"{"v":2}"#,
                "2.0.0".to_string(),
                "v2".to_string(),
            )
            .unwrap();

        manager.store().delete_version("wf-6", "1.0.0").unwrap();
        let versions = manager.store().list_versions("wf-6").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0], "2.0.0");
    }

    #[test]
    fn test_diff_returns_version_range() {
        let manager = make_manager();
        manager
            .create_version(
                "wf-7".to_string(),
                r#"{"v":1}"#,
                "1.0.0".to_string(),
                "v1".to_string(),
            )
            .unwrap();
        manager
            .create_version(
                "wf-7".to_string(),
                r#"{"v":2}"#,
                "2.0.0".to_string(),
                "v2".to_string(),
            )
            .unwrap();

        // diff is not yet implemented (content not stored alongside hash)
        let err = manager.diff("wf-7", "1.0.0", "2.0.0").unwrap_err();
        assert!(matches!(err, VersionError::NotImplemented(_)));
    }

    #[test]
    fn test_version_hash_is_deterministic() {
        let content = r#"{"steps":["a","b","c"]}"#;
        let h1 = calculate_hash(content);
        let h2 = calculate_hash(content);
        assert_eq!(h1, h2);

        // Different content produces different hash
        let h3 = calculate_hash(r#"{"steps":["a","b"]}"#);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(InMemoryVersionStore::new());
        let mut handles = Vec::new();

        for i in 0..10 {
            let store = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                let history = WorkflowHistory::new(format!("wf-conc-{}", i));
                store.save_version(history).unwrap();
                store.load_history(&format!("wf-conc-{}", i)).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // All workflows were saved
        for i in 0..10 {
            let result = store.load_history(&format!("wf-conc-{}", i)).unwrap();
            assert!(result.is_some());
        }
    }

    #[test]
    fn test_create_draft_and_publish() {
        let manager = make_manager();
        let draft = manager
            .create_draft(
                "wf-8".to_string(),
                r#"{"steps":["a"]}"#,
                "1.0.0".to_string(),
                "initial draft".to_string(),
            )
            .unwrap();
        assert!(matches!(draft.status, VersionStatus::Draft));

        let published = manager.publish_version("wf-8", "1.0.0").unwrap();
        assert!(matches!(published.status, VersionStatus::Published));
    }
}
