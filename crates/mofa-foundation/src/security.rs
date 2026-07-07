use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RbacConfig {
    pub roles: HashMap<String, RoleDefinition>,
    pub permissions: HashMap<String, Permission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub inherits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub name: String,
    pub resource: ResourceType,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceType {
    Agent,
    Workflow,
    Tool,
    Data,
    Config,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Read,
    Write,
    Execute,
    Delete,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogConfig {
    pub enabled: bool,
    pub retention_days: u32,
    pub log_actions: Vec<AuditAction>,
    pub storage_backend: StorageBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditAction {
    Login,
    Logout,
    AgentCreate,
    AgentUpdate,
    AgentDelete,
    WorkflowExecute,
    DataAccess,
    ConfigChange,
    PermissionChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageBackend {
    File,
    Database,
    S3,
    CloudWatch,
    Splunk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    pub enabled: bool,
    pub rotation_interval_days: u32,
    pub max_keys_per_user: usize,
    pub require_expiration: bool,
    pub encryption_algorithm: EncryptionAlgorithm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EncryptionAlgorithm {
    AES256,
    RSA,
    Ed25519,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataEncryptionConfig {
    pub at_rest: bool,
    pub in_transit: bool,
    pub key_management: KeyManagementStrategy,
    pub encryption_algorithm: EncryptionAlgorithm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyManagementStrategy {
    AWSKMS,
    GCPKMS,
    AzureKeyVault,
    HashiCorpVault,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReportConfig {
    pub frameworks: Vec<ComplianceFramework>,
    pub report_frequency_days: u32,
    pub include_audit_logs: bool,
    pub include_access_logs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComplianceFramework {
    SOC2,
    HIPAA,
    GDPR,
    PCI,
    ISO27001,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatDetectionConfig {
    pub enabled: bool,
    pub anomaly_detection: bool,
    pub rate_limiting: bool,
    pub suspicious_patterns: Vec<String>,
    pub alert_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub burst_size: u32,
    pub strategy: RateLimitStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RateLimitStrategy {
    TokenBucket,
    LeakyBucket,
    FixedWindow,
    SlidingWindow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub rbac: RbacConfig,
    pub audit_log: AuditLogConfig,
    pub api_keys: ApiKeyConfig,
    pub encryption: DataEncryptionConfig,
    pub compliance: ComplianceReportConfig,
    pub threat_detection: ThreatDetectionConfig,
    pub rate_limiting: RateLimitingConfig,
}

impl Default for RbacConfig {
    fn default() -> Self {
        Self {
            roles: HashMap::new(),
            permissions: HashMap::new(),
        }
    }
}

impl Default for AuditLogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 90,
            log_actions: vec![
                AuditAction::Login,
                AuditAction::Logout,
                AuditAction::AgentCreate,
                AuditAction::AgentDelete,
                AuditAction::WorkflowExecute,
            ],
            storage_backend: StorageBackend::Database,
        }
    }
}

impl Default for ApiKeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rotation_interval_days: 90,
            max_keys_per_user: 5,
            require_expiration: true,
            encryption_algorithm: EncryptionAlgorithm::AES256,
        }
    }
}

impl Default for DataEncryptionConfig {
    fn default() -> Self {
        Self {
            at_rest: true,
            in_transit: true,
            key_management: KeyManagementStrategy::Local,
            encryption_algorithm: EncryptionAlgorithm::AES256,
        }
    }
}

impl Default for ComplianceReportConfig {
    fn default() -> Self {
        Self {
            frameworks: vec![ComplianceFramework::SOC2],
            report_frequency_days: 30,
            include_audit_logs: true,
            include_access_logs: true,
        }
    }
}

impl Default for ThreatDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            anomaly_detection: true,
            rate_limiting: true,
            suspicious_patterns: vec![],
            alert_threshold: 0.8,
        }
    }
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 100,
            burst_size: 20,
            strategy: RateLimitStrategy::SlidingWindow,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            rbac: RbacConfig::default(),
            audit_log: AuditLogConfig::default(),
            api_keys: ApiKeyConfig::default(),
            encryption: DataEncryptionConfig::default(),
            compliance: ComplianceReportConfig::default(),
            threat_detection: ThreatDetectionConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
        }
    }
}
