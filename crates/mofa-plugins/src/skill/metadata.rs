//! Skill 元数据结构
//! Skill metadata structure

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Skill YAML Frontmatter（第1层：启动时加载）
/// Skill YAML Frontmatter (Layer 1: Loaded at startup)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Skill 名称（唯一标识符）
    /// Skill name (Unique identifier)
    pub name: String,
    /// Skill 描述（用于 LLM 判断何时使用）
    /// Skill description (Used by LLM to decide when to use)
    pub description: String,
    /// Skill 分类
    /// Skill category
    #[serde(default)]
    pub category: Option<String>,
    /// 标签
    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// 版本
    /// Version
    #[serde(default)]
    pub version: Option<String>,
    /// 作者
    /// Author
    #[serde(default)]
    pub author: Option<String>,
    /// 是否始终加载（always skills）
    /// Whether to always load (always skills)
    #[serde(default)]
    pub always: bool,
    /// 依赖要求
    /// Dependency requirements
    #[serde(default)]
    pub requires: Option<SkillRequirements>,
    /// 安装指令
    /// Installation instructions
    #[serde(default)]
    pub install: Option<String>,
    /// Ed25519 signature (base64-encoded) covering the markdown body.
    #[serde(default)]
    pub signature: Option<String>,
    /// Ed25519 public key of the signer (base64-encoded).
    #[serde(default)]
    pub signer_key: Option<String>,
}

/// Skill 版本信息
/// Skill version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillVersion {
    /// 内容哈希（SHA256）
    /// Content hash (SHA256)
    pub content_hash: String,
    /// 更新时间
    /// Update time
    pub updated_at: DateTime<Utc>,
}

/// Skill 状态
/// Skill state
#[derive(Debug, Clone, PartialEq)]
pub enum SkillState {
    Active,
    Updating,
    RolledBack,
    Disabled,
}

/// 代码文件定义
/// Code file definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeFile {
    /// 文件路径（相对于 skill 目录）
    /// File path (Relative to skill directory)
    pub path: PathBuf,
    /// 语言类型
    /// Language type
    pub language: String,
    /// 执行命令模板
    /// Execution command template
    pub command: Option<String>,
}

/// Skill 依赖要求
/// Skill dependency requirements
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillRequirements {
    /// 需要的 CLI 工具
    /// Required CLI tools
    #[serde(default)]
    pub cli_tools: Vec<String>,
    /// 需要的环境变量
    /// Required environment variables
    #[serde(default)]
    pub env_vars: Vec<String>,
}

/// 依赖项类型
/// Requirement type
#[derive(Debug, Clone, PartialEq)]
pub enum Requirement {
    /// CLI 工具
    /// CLI tool
    CliTool(String),
    /// 环境变量
    /// Environment variable
    EnvVar(String),
}

/// 依赖检查结果
/// Dependency check result
#[derive(Debug, Clone, Default)]
pub struct RequirementCheck {
    /// 是否满足所有要求
    /// Whether all requirements are met
    pub satisfied: bool,
    /// 缺失的依赖
    /// Missing dependencies
    pub missing: Vec<Requirement>,
}
