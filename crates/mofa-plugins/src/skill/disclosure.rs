//! 渐进式披露控制
//! Progressive disclosure control

use crate::skill::{
    Requirement, RequirementCheck, metadata::SkillMetadata, parser::SkillParser,
    signature::TrustStore,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// 渐进式披露控制器
/// Progressive disclosure controller
///
/// 支持多个 skills 目录按优先级搜索（workspace > builtin > standard）
/// Supports multiple skills directories searched by priority (workspace > builtin > standard)
#[derive(Debug, Clone)]
pub struct DisclosureController {
    /// Skills 目录列表（按优先级排序）
    /// List of skills directories (sorted by priority)
    search_dirs: Vec<PathBuf>,
    /// 缓存的元数据（第1层）
    /// Cached metadata (Layer 1)
    metadata_cache: HashMap<String, SkillMetadata>,
    /// Skill 名称到目录的映射（记录实际来源）
    /// Mapping of skill names to directories (tracking actual source)
    skill_sources: HashMap<String, PathBuf>,
    /// Cryptographic trust store for signature verification.
    trust_store: TrustStore,
}

impl DisclosureController {
    /// 创建新的披露控制器（单目录）
    /// Create a new disclosure controller (single directory)
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            search_dirs: vec![skills_dir.into()],
            metadata_cache: HashMap::new(),
            skill_sources: HashMap::new(),
            trust_store: TrustStore::new(),
        }
    }

    /// 创建新的披露控制器（多目录）
    /// Create a new disclosure controller (multiple directories)
    ///
    /// # Arguments
    ///
    /// * `search_dirs` - Skills 目录列表，按优先级排序（workspace > builtin > standard）
    /// * `search_dirs` - List of skills directories, sorted by priority (workspace > builtin > standard)
    pub fn with_search_dirs(search_dirs: Vec<PathBuf>) -> Self {
        Self {
            search_dirs,
            metadata_cache: HashMap::new(),
            skill_sources: HashMap::new(),
            trust_store: TrustStore::new(),
        }
    }

    /// Returns a reference to the internal trust store so callers can
    /// register trusted public keys before scanning.
    pub fn trust_store(&self) -> &TrustStore {
        &self.trust_store
    }

    /// 查找内置 skills 目录
    /// Find the built-in skills directory
    ///
    /// 按以下顺序查找：
    /// Search in the following order:
    /// 1. CARGO_MANIFEST_DIR/skills（开发时）
    /// 1. CARGO_MANIFEST_DIR/skills (during development)
    /// 2. 可执行文件父目录/skills（已安装）
    /// 2. Executable parent directory/skills (when installed)
    /// 3. /usr/local/lib/mofa/skills（标准安装路径）
    /// 3. /usr/local/lib/mofa/skills (standard installation path)
    pub fn find_builtin_skills() -> Option<PathBuf> {
        // Try CARGO_MANIFEST_DIR first (development)
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let skills_path = PathBuf::from(manifest_dir).join("skills");
            if skills_path.exists() {
                return Some(skills_path);
            }
        }

        // Try executable parent directory (installed binary)
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let skills_path = parent.join("skills");
            if skills_path.exists() {
                return Some(skills_path);
            }
        }

        // Try grandparent directory (for /usr/local/bin/mofa -> /usr/local/lib/mofa/skills)
        if let Ok(exe) = std::env::current_exe()
            && let Some(grandparent) = exe.parent().and_then(|p| p.parent())
        {
            let skills_path = grandparent.join("lib").join("mofa").join("skills");
            if skills_path.exists() {
                return Some(skills_path);
            }
        }

        // Try standard installation path
        let standard_path = PathBuf::from("/usr/local/lib/mofa/skills");
        if standard_path.exists() {
            return Some(standard_path);
        }

        None
    }

    /// 扫描并加载所有 Skills 的元数据（第1层）
    /// Scan and load metadata for all skills (Layer 1)
    ///
    /// 按优先级从多个目录扫描，先找到的优先
    /// Scan multiple directories by priority; the first match takes precedence
    pub fn scan_metadata(&mut self) -> mofa_kernel::plugin::PluginResult<usize> {
        let mut count = 0;

        for skills_dir in &self.search_dirs {
            if !skills_dir.exists() {
                continue;
            }

            for entry in walkdir::WalkDir::new(skills_dir)
                .min_depth(1)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_dir() {
                    let skill_name = entry.file_name().to_string_lossy().to_string();

                    // Skip if already found from higher priority dir
                    if self.metadata_cache.contains_key(&skill_name) {
                        continue;
                    }

                    let skill_md = entry.path().join("SKILL.md");
                    if skill_md.exists()
                        && let Ok((metadata, markdown)) =
                            SkillParser::parse_from_file(&skill_md)
                    {
                        // ── Signature verification ──────────────────
                        if let Some(sig) = &metadata.signature {
                            if let Some(key) = &metadata.signer_key {
                                if let Err(e) =
                                    self.trust_store.verify(markdown.trim(), sig, key)
                                {
                                    tracing::warn!(
                                        "Skill '{}' rejected — signature verification failed: {}",
                                        skill_name,
                                        e
                                    );
                                    continue;
                                }
                            } else {
                                tracing::warn!(
                                    "Skill '{}' rejected — has signature but no signer_key",
                                    skill_name
                                );
                                continue;
                            }
                        }
                        // ── End verification ────────────────────────

                        self.metadata_cache.insert(metadata.name.clone(), metadata);
                        self.skill_sources.insert(skill_name, skills_dir.clone());
                        count += 1;
                    }
                }
            }
        }

        tracing::info!(
            "Scanned {} skills from {} directories",
            count,
            self.search_dirs.len()
        );
        Ok(count)
    }

    /// 第1层：获取所有 Skills 的元数据（用于系统提示）
    /// Layer 1: Get metadata for all skills (for system prompts)
    pub fn get_all_metadata(&self) -> Vec<SkillMetadata> {
        self.metadata_cache.values().cloned().collect()
    }

    /// 构建系统提示（仅包含元数据）
    /// Build the system prompt (metadata only)
    pub fn build_system_prompt(&self) -> String {
        let metadata: Vec<String> = self
            .metadata_cache
            .values()
            .map(|m| format!("- {}: {}", m.name, m.description))
            .collect();

        format!(
            "You have access to the following skills:\n{}\n\nWhen a task requires a specific skill, \
             load the full SKILL.md file to get detailed instructions.",
            metadata.join("\n")
        )
    }

    /// 获取 Skill 目录路径
    /// Get the skill directory path
    pub fn get_skill_path(&self, name: &str) -> Option<PathBuf> {
        self.skill_sources.get(name).map(|dir| dir.join(name))
    }

    /// 检查 Skill 是否存在
    /// Check if a skill exists
    pub fn has_skill(&self, name: &str) -> bool {
        self.metadata_cache.contains_key(name)
    }

    /// 获取标记为 always 的技能名称列表
    /// Get the list of skill names marked as "always"
    pub fn get_always_skills(&self) -> Vec<String> {
        self.metadata_cache
            .values()
            .filter(|m| m.always)
            .map(|m| m.name.clone())
            .collect()
    }

    /// 检查技能依赖是否满足
    /// Check if skill requirements are satisfied
    pub fn check_requirements(&self, name: &str) -> RequirementCheck {
        let metadata = match self.metadata_cache.get(name) {
            Some(m) => m,
            None => return RequirementCheck::default(),
        };

        let requires = metadata.requires.as_ref();
        let mut missing = Vec::new();

        if let Some(reqs) = requires {
            // Check CLI tools
            for tool in &reqs.cli_tools {
                if !Self::command_exists(tool) {
                    missing.push(Requirement::CliTool(tool.clone()));
                }
            }

            // Check environment variables
            for env_var in &reqs.env_vars {
                if std::env::var(env_var).is_err() {
                    missing.push(Requirement::EnvVar(env_var.clone()));
                }
            }
        }

        RequirementCheck {
            satisfied: missing.is_empty(),
            missing,
        }
    }

    /// 获取技能的安装指令
    /// Get installation instructions for a skill
    pub fn get_install_instructions(&self, name: &str) -> Option<String> {
        self.metadata_cache
            .get(name)
            .and_then(|m| m.install.as_ref())
            .cloned()
    }

    /// 获取缺失依赖的描述字符串
    /// Get a description string of missing requirements
    pub fn get_missing_requirements_description(&self, name: &str) -> String {
        let check = self.check_requirements(name);
        if check.satisfied {
            String::new()
        } else {
            check
                .missing
                .iter()
                .map(|r| match r {
                    Requirement::CliTool(t) => format!("CLI: {}", t),
                    Requirement::EnvVar(v) => format!("ENV: {}", v),
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    /// 检查命令是否存在
    /// Check if a command exists
    fn command_exists(cmd: &str) -> bool {
        which::which(cmd).is_ok()
    }

    /// 按关键词搜索相关 Skills
    /// Search for related skills by keyword
    pub fn search(&self, query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();

        self.metadata_cache
            .values()
            .filter(|m| {
                m.name.to_lowercase().contains(&query_lower)
                    || m.description.to_lowercase().contains(&query_lower)
                    || m.tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .map(|m| m.name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_test_skill(dir: &Path, name: &str, description: &str) -> std::io::Result<()> {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir)?;

        let content = format!(
            r#"---
name: {}
description: {}
category: test
tags: [test]
version: "1.0.0"
---

# {} Skill

This is a test skill."#,
            name, description, name
        );

        fs::write(skill_dir.join("SKILL.md"), content)?;
        Ok(())
    }

    #[test]
    fn test_scan_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path();

        create_test_skill(skills_dir, "skill1", "First skill").unwrap();
        create_test_skill(skills_dir, "skill2", "Second skill").unwrap();

        let mut controller = DisclosureController::new(skills_dir);
        let count = controller.scan_metadata().unwrap();

        assert_eq!(count, 2);
        assert!(controller.has_skill("skill1"));
        assert!(controller.has_skill("skill2"));
        assert!(!controller.has_skill("skill3"));
    }

    #[test]
    fn test_build_system_prompt() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path();

        create_test_skill(skills_dir, "skill1", "First skill").unwrap();
        create_test_skill(skills_dir, "skill2", "Second skill").unwrap();

        let mut controller = DisclosureController::new(skills_dir);
        controller.scan_metadata().unwrap();

        let prompt = controller.build_system_prompt();
        assert!(prompt.contains("skill1"));
        assert!(prompt.contains("First skill"));
        assert!(prompt.contains("skill2"));
        assert!(prompt.contains("Second skill"));
    }

    #[test]
    fn test_search() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path();

        create_test_skill(skills_dir, "pdf_processing", "Process PDF files").unwrap();
        create_test_skill(skills_dir, "web_scraping", "Scrape web pages").unwrap();

        let mut controller = DisclosureController::new(skills_dir);
        controller.scan_metadata().unwrap();

        let results = controller.search("pdf");
        assert_eq!(results, vec!["pdf_processing".to_string()]);

        let results = controller.search("web");
        assert_eq!(results, vec!["web_scraping".to_string()]);
    }
}
