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

    /// Create with default allowed commands (safe, read-only commands only)
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
        ])
    }

    fn is_command_allowed(&self, command: &str) -> bool {
        if self.allowed_commands.is_empty() {
            return false; // Default deny if no whitelist
        }
        self.allowed_commands
            .iter()
            .any(|allowed| command == allowed)
    }

    /// Validate that command arguments don't contain dangerous flags or patterns.
    fn validate_args(command: &str, args: &[String]) -> Result<(), String> {
        // Dangerous argument flags that enable arbitrary command execution
        const DANGEROUS_FLAGS: &[&str] = &[
            "-exec",
            "-execdir",
            "--exec",
            "-delete",
            "-ok",
            "-okdir",
        ];

        // Dangerous shell metacharacters in arguments
        const DANGEROUS_PATTERNS: &[&str] = &[
            "|", ";", "&&", "||", "`", "$(", "${",
            ">", ">>", "<",
        ];

        for arg in args {
            // Check for dangerous flags
            let arg_lower = arg.to_lowercase();
            for flag in DANGEROUS_FLAGS {
                if arg_lower == *flag {
                    return Err(format!(
                        "Dangerous flag '{}' is not allowed for command '{}'",
                        arg, command
                    ));
                }
            }

            // Check for shell metacharacters
            for pattern in DANGEROUS_PATTERNS {
                if arg.contains(pattern) {
                    return Err(format!(
                        "Argument '{}' contains dangerous shell metacharacter '{}'",
                        arg, pattern
                    ));
                }
            }
        }

        Ok(())
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

        // Validate arguments for dangerous flags and patterns
        if let Err(reason) = Self::validate_args(command, &args) {
            return Err(anyhow::anyhow!(
                "Argument validation failed: {}",
                reason
            ));
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
