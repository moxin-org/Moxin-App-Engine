use super::*;
use serde_json::json;
use tokio::process::Command;

/// Shell 命令工具 - 执行系统命令（受限）
/// Shell command tool - Execute system commands (restricted)
pub struct ShellCommandTool {
    definition: ToolDefinition,
    allowed_commands: Vec<String>,
}

impl ShellCommandTool {
    pub fn new(allowed_commands: Vec<String>) -> Self {
        Self {
            definition: ToolDefinition {
                name: "shell".to_string(),
                description:
                    "Execute shell commands. Only whitelisted commands are allowed for security."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        },
                        "args": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Command arguments"
                        },
                        "working_dir": {
                            "type": "string",
                            "description": "Working directory for command execution"
                        }
                    },
                    "required": ["command"]
                }),
                requires_confirmation: true,
            },
            allowed_commands,
        }
    }

    /// Create with default allowed commands
    pub fn new_with_defaults() -> Self {
        Self::new(vec![
            "ls".to_string(),
            "pwd".to_string(),
            "echo".to_string(),
            "date".to_string(),
            "whoami".to_string(),
            "head".to_string(),
            "tail".to_string(),
            "wc".to_string(),
            "grep".to_string(),
        ])
    }

    fn is_command_allowed(&self, command: &str) -> bool {
        if self.allowed_commands.is_empty() {
            return false; // Default deny if no whitelist
        }
        self.allowed_commands
            .iter()
            .any(|allowed| command == allowed || command.starts_with(&format!("{} ", allowed)))
    }

    /// Check for dangerous arguments like redirection or piping
    fn has_dangerous_args(args: &[String]) -> bool {
        let dangerous_patterns = ["|", ">", "<", "$", "`", ";", "&", ">>", "2>", "&>"];
        args.iter().any(|arg| {
            dangerous_patterns.iter().any(|pattern| arg.contains(pattern))
        })
    }
}

#[async_trait::async_trait]
impl ToolExecutor for ShellCommandTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value> {
        let command = arguments["command"].as_str().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed("Command is required".to_string())
        })?;

        if !self.is_command_allowed(command) {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Command '{}' is not in the allowed commands list. Allowed: {:?}",
                command, self.allowed_commands
            )));
        }

        let args: Vec<String> = arguments
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if Self::has_dangerous_args(&args) {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Dangerous shell operators detected in arguments: {:?}",
                args
            )));
        }

        let mut cmd = Command::new(command);
        cmd.args(&args);

        if let Some(dir) = arguments.get("working_dir").and_then(|d| d.as_str()) {
            cmd.current_dir(dir);
        }

        let output = cmd.output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let truncate = |s: String, limit: usize| -> String {
            if s.len() > limit {
                // Find the last valid char boundary at or before the limit
                // to avoid panicking on multi-byte UTF-8 characters.
                let mut end = limit;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...[truncated]", &s[..end])
            } else {
                s
            }
        };

        Ok(json!({
            "success": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": truncate(stdout, 5000),
            "stderr": truncate(stderr, 5000)
        }))
    }
}
