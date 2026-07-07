//! `mofa agent logs` command implementation

use crate::CliError;
use crate::context::CliContext;
use colored::Colorize;
use std::collections::VecDeque;
use std::io::SeekFrom;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::time::{Duration, interval};

/// Execute the `mofa agent logs` command
pub async fn run(
    ctx: &CliContext,
    agent_id: &str,
    tail: bool,
    level: Option<String>,
    grep: Option<String>,
    limit: Option<usize>,
    json: bool,
) -> Result<(), CliError> {
    // Determine log file location
    let log_file = get_agent_log_path(&ctx.data_dir, agent_id);

    // Check if log file exists
    if !log_file.exists() {
        // Check if agent exists in registry
        let agent_exists = ctx.persistent_agents.exists(agent_id).await;
        if agent_exists {
            println!(
                "{} Agent '{}' exists but has no logs yet.",
                "ℹ".yellow(),
                agent_id.cyan()
            );
            println!(
                "  {}",
                "Logs will appear here once the agent starts producing output.".bright_black()
            );

            if tail {
                println!(
                    "\n{} Waiting for logs... (Press Ctrl+C to exit)",
                    "→".green()
                );
                wait_for_file_and_tail(&log_file).await?;
            }
            return Ok(());
        } else {
            // Log file doesn't exist and agent isn't registered
            return Err(CliError::StateError(format!(
                "Agent '{}' not found in registry and no log file exists.\n  Log file location: {}",
                agent_id,
                log_file.display()
            )));
        }
    }

    if tail {
        println!(
            "{} Tailing logs for agent: {}",
            "→".green(),
            agent_id.cyan()
        );
        if level.is_some() || grep.is_some() {
            println!("  {} Filters active", "•".bright_black());
        }
        println!("  (Press Ctrl+C to exit)\n");
        tail_log_file(&log_file, &level, &grep).await?;
    } else {
        println!(
            "{} Displaying recent logs for agent: {}\n",
            "→".green(),
            agent_id.cyan()
        );
        if let Some(lvl) = &level {
            println!("  {} Level filter: {}", "•".bright_black(), lvl.cyan());
        }
        if let Some(pattern) = &grep {
            println!("  {} Search: {}", "•".bright_black(), pattern.cyan());
        }
        if let Some(n) = limit {
            println!("  {} Limit: {} lines", "•".bright_black(), n);
        }
        println!();
        display_log_file(&log_file, &level, &grep, limit, json).await?;
    }

    Ok(())
}

/// Get the path to an agent's log file
fn get_agent_log_path(data_dir: &std::path::Path, agent_id: &str) -> std::path::PathBuf {
    data_dir.join("logs").join(format!("{}.log", agent_id))
}

/// Display entire log file contents with optional filtering
async fn display_log_file(
    log_path: &std::path::Path,
    level: &Option<String>,
    grep: &Option<String>,
    limit: Option<usize>,
    json: bool,
) -> Result<(), CliError> {
    let file = File::open(log_path).await.map_err(|e| {
        CliError::StateError(format!(
            "Failed to open log file {}: {}",
            log_path.display(),
            e
        ))
    })?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut output_lines = VecDeque::new();

    while let Some(line) = lines.next_line().await.map_err(|e| {
        CliError::StateError(format!(
            "Failed to read log file {}: {}",
            log_path.display(),
            e
        ))
    })? {
        // Apply filters
        if let Some(level_filter) = level
            && !matches_log_level(&line, level_filter)
        {
            continue;
        }

        if let Some(pattern) = grep
            && !line.to_lowercase().contains(&pattern.to_lowercase())
        {
            continue;
        }

        push_with_limit(&mut output_lines, line, limit);
    }

    let count = output_lines.len();

    // Output
    if json {
        let json_output = serde_json::json!({
            "agent_id": log_path.file_stem().and_then(|s| s.to_str()),
            "lines": output_lines,
            "count": count
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        for line in output_lines {
            println!("{}", colorize_log_line(&line));
        }
    }

    Ok(())
}

fn push_with_limit(buffer: &mut VecDeque<String>, line: String, limit: Option<usize>) {
    match limit {
        Some(0) => {}
        Some(max) => {
            if buffer.len() == max {
                buffer.pop_front();
            }
            buffer.push_back(line);
        }
        None => buffer.push_back(line),
    }
}

/// Colorize log line based on log level
fn colorize_log_line(line: &str) -> String {
    let upper = line.to_uppercase();
    if upper.contains("ERROR") || upper.contains(" FATAL ") {
        line.red().to_string()
    } else if upper.contains("WARN") || upper.contains(" WARNING ") {
        line.yellow().to_string()
    } else if upper.contains("INFO") {
        line.green().to_string()
    } else if upper.contains("DEBUG") || upper.contains("TRACE") {
        line.bright_black().to_string()
    } else {
        line.to_string()
    }
}

/// Check if log line matches the specified level
fn matches_log_level(line: &str, level: &str) -> bool {
    let upper_line = line.to_uppercase();
    let upper_level = level.to_uppercase();
    match upper_level.as_str() {
        "ERROR" | "FATAL" => upper_line.contains("ERROR") || upper_line.contains("FATAL"),
        "WARN" | "WARNING" => upper_line.contains("WARN"),
        "INFO" => upper_line.contains("INFO"),
        "DEBUG" => upper_line.contains("DEBUG"),
        "TRACE" => upper_line.contains("TRACE"),
        _ => true, // Unknown level, show all
    }
}

/// Tail log file, following new content as it appears
async fn tail_log_file(
    log_path: &std::path::Path,
    level: &Option<String>,
    grep: &Option<String>,
) -> Result<(), CliError> {
    let mut file = File::open(log_path).await.map_err(|e| {
        CliError::StateError(format!(
            "Failed to open log file {}: {}",
            log_path.display(),
            e
        ))
    })?;

    // Start at the end of file
    let file_len = file
        .metadata()
        .await
        .map_err(|e| CliError::StateError(format!("Failed to get file metadata: {}", e)))?
        .len();

    file.seek(SeekFrom::Start(file_len))
        .await
        .map_err(|e| CliError::StateError(format!("Failed to seek to end of file: {}", e)))?;

    let mut reader = BufReader::new(file);
    let mut interval = interval(Duration::from_millis(100));
    let mut last_pos = file_len;

    loop {
        interval.tick().await;

        // Check if file has been rotated (size decreased or file recreated)
        let current_metadata = match tokio::fs::metadata(log_path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File was deleted/rotated, wait for it to reappear
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            Err(e) => {
                return Err(CliError::StateError(format!(
                    "Failed to check log file status: {}",
                    e
                )));
            }
        };

        let current_size = current_metadata.len();

        // Handle log rotation
        if current_size < last_pos {
            // File was truncated or rotated - reopen from beginning
            drop(reader);
            let new_file = File::open(log_path).await.map_err(|e| {
                CliError::StateError(format!("Failed to reopen rotated log file: {}", e))
            })?;
            reader = BufReader::new(new_file);
            last_pos = 0;
        }

        // Read new lines
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    // Apply filters
                    let should_show = {
                        let level_match = if let Some(level_filter) = level {
                            matches_log_level(&line, level_filter)
                        } else {
                            true
                        };

                        let grep_match = if let Some(pattern) = grep {
                            line.to_lowercase().contains(&pattern.to_lowercase())
                        } else {
                            true
                        };

                        level_match && grep_match
                    };

                    if should_show {
                        print!("{}", colorize_log_line(&line));
                    }
                    last_pos += line.len() as u64;
                }
                Err(e) => {
                    return Err(CliError::StateError(format!(
                        "Failed to read from log file: {}",
                        e
                    )));
                }
            }
        }
    }
}

/// Wait for a log file to be created and then start tailing it
async fn wait_for_file_and_tail(log_path: &std::path::Path) -> Result<(), CliError> {
    let mut interval = interval(Duration::from_millis(500));

    // Wait for file to exist
    loop {
        interval.tick().await;
        if log_path.exists() {
            break;
        }
    }

    println!("{} Log file created, starting tail...\n", "✓".green());
    tail_log_file(log_path, &None, &None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::CliContext;
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_display_log_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_file = temp_dir.path().join("test.log");

        // Write test content
        let mut file = File::create(&log_file).await.unwrap();
        file.write_all(b"Line 1\nLine 2\nLine 3\n").await.unwrap();
        file.flush().await.unwrap();
        drop(file);

        // Test reading
        display_log_file(&log_file, &None, &None, None, false)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_get_agent_log_path() {
        let data_dir = std::path::Path::new("/tmp/mofa");
        let log_path = get_agent_log_path(data_dir, "agent-123");
        assert_eq!(
            log_path,
            std::path::Path::new("/tmp/mofa/logs/agent-123.log")
        );
    }

    #[tokio::test]
    async fn test_logs_command_with_existing_file() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        // Create an agent in registry
        use crate::state::agent_state::AgentMetadata;
        let metadata = AgentMetadata::new("test-agent".to_string(), "Test Agent".to_string());
        ctx.persistent_agents.register(metadata).await.unwrap();

        // Create log file with content
        let log_file = get_agent_log_path(&ctx.data_dir, "test-agent");
        tokio::fs::create_dir_all(log_file.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&log_file, b"[2024-01-01 10:00:00] INFO: Agent started\n[2024-01-01 10:00:01] DEBUG: Processing request\n")
            .await
            .unwrap();

        // Test reading logs
        let result = run(&ctx, "test-agent", false, None, None, None, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_logs_command_missing_agent() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        // Try to read logs for non-existent agent
        let result = run(&ctx, "nonexistent-agent", false, None, None, None, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_logs_command_no_logs_yet() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        // Create an agent in registry
        use crate::state::agent_state::AgentMetadata;
        let metadata = AgentMetadata::new("new-agent".to_string(), "New Agent".to_string());
        ctx.persistent_agents.register(metadata).await.unwrap();

        // Try to read logs for agent with no logs yet
        let result = run(&ctx, "new-agent", false, None, None, None, false).await;
        // Should succeed but show message about no logs
        assert!(result.is_ok());
    }

    #[test]
    fn test_colorize_log_line_dims_trace_prefix_without_spaces() {
        let line = "TRACE module::op starting";
        assert_eq!(colorize_log_line(line), line.bright_black().to_string());
    }

    #[test]
    fn test_colorize_log_line_dims_bracketed_trace_prefix() {
        let line = "[TRACE] module::op starting";
        assert_eq!(colorize_log_line(line), line.bright_black().to_string());
    }

    #[test]
    fn test_push_with_limit_keeps_most_recent_lines() {
        let mut buffer = VecDeque::new();
        push_with_limit(&mut buffer, "line-1".to_string(), Some(2));
        push_with_limit(&mut buffer, "line-2".to_string(), Some(2));
        push_with_limit(&mut buffer, "line-3".to_string(), Some(2));

        let collected: Vec<_> = buffer.into_iter().collect();
        assert_eq!(collected, vec!["line-2", "line-3"]);
    }

    #[test]
    fn test_push_with_zero_limit_keeps_no_lines() {
        let mut buffer = VecDeque::new();
        push_with_limit(&mut buffer, "line-1".to_string(), Some(0));
        assert!(buffer.is_empty());
    }
}
