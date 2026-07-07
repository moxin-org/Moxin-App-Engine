//! Prompt Context - Specialized context for prompt building
//!
//! This module provides prompt-building capabilities as a specialization
//! of the core context system. It uses RichAgentContext for storage
//! while providing domain-specific prompt building functionality.
//!
//! # Design
//!
//! - Uses `RichAgentContext` for context management
//! - Provides fluent builder API for prompt construction
//! - Supports bootstrap file loading and memory integration
//! - Progressive disclosure for skills/tools
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::agent::context::prompt::PromptContextBuilder;
//! use mofa_kernel::agent::context::CoreAgentContext;
//!
//! let ctx = PromptContextBuilder::new("/path/to/workspace")
//!     .with_name("MyAgent")
//!     .build()
//!     .await?;
//!
//! let prompt = ctx.build_system_prompt().await?;
//! ```

use chrono::Utc;
use std::path::{Path, PathBuf};
use tokio::fs;

use super::rich::RichAgentContext;
use crate::agent::components::memory::FileBasedStorage;
use mofa_kernel::agent::context::AgentContext;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use std::sync::Arc;

/// Agent identity information
#[derive(Debug, Clone)]
pub struct AgentIdentity {
    /// Agent name
    pub name: String,
    /// Agent description
    pub description: String,
    /// Emoji icon
    pub icon: Option<String>,
}

impl Default for AgentIdentity {
    fn default() -> Self {
        Self {
            name: "Agent".to_string(),
            description: "A helpful AI assistant".to_string(),
            icon: None,
        }
    }
}

/// Context builder for agent system prompts
///
/// This component standardizes how agents build their system prompts by:
/// - Loading bootstrap files from the workspace
/// - Injecting memory context
/// - Managing skill loading and progressive disclosure
/// - Providing a consistent identity section
///
/// Uses `RichAgentContext` internally for context management.
pub struct PromptContext {
    /// Base workspace directory
    workspace: PathBuf,
    /// Agent identity (name, description)
    identity: AgentIdentity,
    /// Bootstrap files to load
    bootstrap_files: Vec<String>,
    /// Memory storage (lazy initialized)
    memory: Option<Arc<FileBasedStorage>>,
    /// Always-loaded skills/tools
    always_load: Vec<String>,
    /// Agent name for display
    agent_name: String,
    /// Rich context for extended functionality
    rich_ctx: RichAgentContext,
    /// Optional inline system prompt. When set, overrides workspace-based prompt building.
    inline_prompt: Option<String>,
}

impl PromptContext {
    /// Default bootstrap files to load
    pub fn default_bootstrap_files() -> Vec<String> {
        vec![
            "AGENTS.md".to_string(),
            "SOUL.md".to_string(),
            "USER.md".to_string(),
            "TOOLS.md".to_string(),
            "IDENTITY.md".to_string(),
        ]
    }

    /// Create a new prompt context
    pub async fn new(workspace: impl AsRef<Path>) -> AgentResult<Self> {
        let workspace = workspace.as_ref().to_path_buf();
        let core_ctx = AgentContext::new(format!("prompt-{}", uuid::Uuid::new_v4()));
        let rich_ctx = RichAgentContext::new(core_ctx);

        Ok(Self {
            workspace,
            identity: AgentIdentity::default(),
            bootstrap_files: Self::default_bootstrap_files(),
            memory: None,
            always_load: Vec::new(),
            agent_name: "agent".to_string(),
            rich_ctx,
            inline_prompt: None,
        })
    }

    /// Create with custom identity
    pub async fn with_identity(
        workspace: impl AsRef<Path>,
        identity: AgentIdentity,
    ) -> AgentResult<Self> {
        let workspace = workspace.as_ref().to_path_buf();
        let agent_name = identity.name.clone();
        let core_ctx = AgentContext::new(format!("prompt-{}", uuid::Uuid::new_v4()));
        let rich_ctx = RichAgentContext::new(core_ctx);

        Ok(Self {
            workspace,
            identity,
            bootstrap_files: Self::default_bootstrap_files(),
            memory: None,
            always_load: Vec::new(),
            agent_name,
            rich_ctx,
            inline_prompt: None,
        })
    }

    /// Set the bootstrap files to load
    pub fn with_bootstrap_files(mut self, files: Vec<String>) -> Self {
        self.bootstrap_files = files;
        self
    }

    /// Replace the agent identity.
    pub fn set_identity(&mut self, identity: AgentIdentity) {
        self.agent_name = identity.name.clone();
        self.identity = identity;
    }

    /// Replace the bootstrap file list.
    pub fn set_bootstrap_files(&mut self, files: Vec<String>) {
        self.bootstrap_files = files;
    }

    /// Set skills that should always be loaded
    pub fn with_always_load(mut self, skills: Vec<String>) -> Self {
        self.always_load = skills;
        self
    }

    /// Initialize memory storage (lazy)
    async fn init_memory(&mut self) -> AgentResult<()> {
        if self.memory.is_none() {
            self.memory = Some(Arc::new(
                FileBasedStorage::new(&self.workspace).await.map_err(|e| {
                    AgentError::MemoryError(format!("Failed to init memory: {}", e))
                })?,
            ));
        }
        Ok(())
    }

    /// Override the system prompt with a fixed inline string.
    ///
    /// When set, `build_system_prompt` returns this string directly instead of
    /// assembling the prompt from workspace bootstrap files and memory.
    pub fn set_inline_prompt(&mut self, prompt: String) {
        self.inline_prompt = Some(prompt);
    }

    /// Build the complete system prompt
    pub async fn build_system_prompt(&mut self) -> AgentResult<String> {
        // Return the inline override immediately if one has been set.
        if let Some(ref prompt) = self.inline_prompt {
            return Ok(prompt.clone());
        }

        let mut parts = Vec::new();

        // 1. Core identity
        parts.push(self.get_identity_section());

        // 2. Bootstrap files
        let bootstrap = self.load_bootstrap_files().await?;
        if !bootstrap.is_empty() {
            parts.push(bootstrap);
        }

        // 3. Memory context (lazy init)
        if self.init_memory().await.is_err() {
            // Memory is optional, continue without it
        } else if let Some(memory) = &self.memory
            && let Ok(memory_context) = memory.get_memory_context().await
            && !memory_context.is_empty()
        {
            parts.push(format!("# Memory\n\n{}", memory_context));
        }

        // 4. Record that we built a prompt (using rich context)
        self.rich_ctx
            .record_output(
                "prompt_builder",
                serde_json::json!({
                    "prompt_length": parts.join("\n\n---\n\n").len(),
                    "bootstrap_files": self.bootstrap_files.len(),
                }),
            )
            .await;

        Ok(parts.join("\n\n---\n\n"))
    }

    /// Get the identity section of the system prompt
    fn get_identity_section(&self) -> String {
        let now = Utc::now().format("%Y-%m-%d %H:%M (%A)");
        let workspace_path = self.workspace.display();
        let icon = self.identity.icon.as_deref().unwrap_or("");
        let description = if self.identity.description.is_empty() {
            "a helpful AI assistant"
        } else {
            &self.identity.description
        };

        format!(
            r#"# {} {} {}

You are {}, {}.

## Current Time
{}

## Workspace
Your workspace is at: {}
- Memory files: {}/memory/MEMORY.md
- Daily notes: {}/memory/YYYY-MM-DD.md
- Custom skills: {}/skills/{{{{skill-name}}}}/SKILL.md

Always be helpful, accurate, and concise. When using tools, explain what you're doing.
When remembering something, write to {}/memory/MEMORY.md"#,
            icon,
            self.identity.name,
            description,
            self.identity.name,
            description,
            now,
            workspace_path,
            workspace_path,
            workspace_path,
            workspace_path,
            workspace_path
        )
    }

    /// Load bootstrap files from workspace
    async fn load_bootstrap_files(&self) -> AgentResult<String> {
        let mut parts = Vec::new();

        for filename in &self.bootstrap_files {
            let file_path = self.workspace.join(filename);
            if file_path.exists()
                && let Ok(content) = fs::read_to_string(&file_path).await
            {
                parts.push(format!("## {}\n\n{}", filename, content));
            }
        }

        Ok(parts.join("\n\n"))
    }

    /// Get memory storage reference
    pub async fn memory(&mut self) -> AgentResult<&FileBasedStorage> {
        self.init_memory().await?;
        Ok(self.memory.as_ref().unwrap())
    }

    /// Get the workspace path
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Get the rich context for extended functionality
    pub fn rich_context(&self) -> &RichAgentContext {
        &self.rich_ctx
    }

    /// Get the identity
    pub fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
}

/// Builder for creating PromptContext with fluent API
pub struct PromptContextBuilder {
    workspace: PathBuf,
    identity: AgentIdentity,
    bootstrap_files: Vec<String>,
    always_load: Vec<String>,
}

impl PromptContextBuilder {
    /// Create a new builder
    pub fn new(workspace: impl AsRef<Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            identity: AgentIdentity::default(),
            bootstrap_files: PromptContext::default_bootstrap_files(),
            always_load: Vec::new(),
        }
    }

    /// Set the agent identity
    pub fn with_identity(mut self, identity: AgentIdentity) -> Self {
        self.identity = identity;
        self
    }

    /// Set the agent name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.identity.name = name.into();
        self
    }

    /// Set the agent description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.identity.description = description.into();
        self
    }

    /// Set the agent icon
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.identity.icon = Some(icon.into());
        self
    }

    /// Set bootstrap files
    pub fn with_bootstrap_files(mut self, files: Vec<String>) -> Self {
        self.bootstrap_files = files;
        self
    }

    /// Set always-loaded skills
    pub fn with_always_load(mut self, skills: Vec<String>) -> Self {
        self.always_load = skills;
        self
    }

    /// Build the PromptContext
    pub async fn build(self) -> AgentResult<PromptContext> {
        let agent_name = self.identity.name.clone();
        PromptContext::with_identity(&self.workspace, self.identity)
            .await
            .map(|mut ctx| {
                ctx.bootstrap_files = self.bootstrap_files;
                ctx.always_load = self.always_load;
                ctx.agent_name = agent_name;
                ctx
            })
    }
}

impl Clone for PromptContext {
    fn clone(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            identity: self.identity.clone(),
            bootstrap_files: self.bootstrap_files.clone(),
            memory: self.memory.clone(),
            always_load: self.always_load.clone(),
            agent_name: self.agent_name.clone(),
            rich_ctx: RichAgentContext::new(self.rich_ctx.inner().clone()),
            inline_prompt: self.inline_prompt.clone(),
        }
    }
}
