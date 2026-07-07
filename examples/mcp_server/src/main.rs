//! MCP Server Example
//!
//! Exposes MoFA agent internals as an MCP server so external AI clients
//! (Claude Desktop, Cursor, etc.) can directly interact with a running
//! MoFA agent over HTTP/SSE.
//!
//! # Running
//!
//! ```bash
//! cargo run -p mcp_server
//! PORT=8080 cargo run -p mcp_server
//! ```
//!
//! # Connecting from Claude Desktop
//!
//! Add to `claude_desktop_config.json`:
//! ```json
//! {
//!   "mcpServers": {
//!     "mofa-agent": {
//!       "url": "http://127.0.0.1:3000/mcp"
//!     }
//!   }
//! }
//! ```
//!
//! # Tools exposed
//!
//! | Tool | Description |
//! |------|-------------|
//! | `health_check` | Server uptime, platform info, invocation count |
//! | `memory_write` | Store a key-value pair in the agent's memory |
//! | `memory_read` | Retrieve a value from the agent's memory |
//! | `memory_list` | List all keys in the agent's memory |
//! | `agent_run` | Run the built-in MoFA agent with a task |

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use mofa_foundation::agent::components::memory::{InMemoryStorage, Memory, MemoryValue};
use mofa_foundation::agent::tools::mcp::McpServerManager;
use mofa_kernel::agent::components::mcp::McpHostConfig;
use mofa_kernel::agent::components::tool::{ToolExt, ToolInput, ToolResult};
use mofa_kernel::agent::context::AgentContext;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

// ============================================================================
// Shared agent state -- all tools read/write the same memory and counters
// ============================================================================

struct AgentState {
    memory: Mutex<InMemoryStorage>,
    start_time: SystemTime,
    invocations: AtomicU64,
}

impl AgentState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            memory: Mutex::new(InMemoryStorage::new()),
            start_time: SystemTime::now(),
            invocations: AtomicU64::new(0),
        })
    }

    fn tick(&self) -> u64 {
        self.invocations.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn uptime_secs(&self) -> u64 {
        self.start_time
            .elapsed()
            .unwrap_or_default()
            .as_secs()
    }
}

// ============================================================================
// Tool: health_check
// ============================================================================

struct HealthCheckTool {
    state: Arc<AgentState>,
}

#[async_trait::async_trait]
impl mofa_kernel::agent::components::tool::Tool for HealthCheckTool {
    fn name(&self) -> &str {
        "health_check"
    }

    fn description(&self) -> &str {
        "Returns the health status of this MoFA MCP server: uptime in seconds, \
         platform details, total tool invocations since start, and framework version. \
         Use this first to verify the agent is reachable."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _input: ToolInput, _ctx: &AgentContext) -> ToolResult {
        let n = self.state.tick();
        ToolResult::success(serde_json::json!({
            "status": "healthy",
            "framework": "MoFA",
            "uptime_seconds": self.state.uptime_secs(),
            "total_invocations": n,
            "platform": {
                "os":          std::env::consts::OS,
                "arch":        std::env::consts::ARCH,
                "cpu_threads": std::thread::available_parallelism()
                                   .map(|n| n.get()).unwrap_or(0),
            },
        }))
    }
}

// ============================================================================
// Tool: memory_write
// ============================================================================

struct MemoryWriteTool {
    state: Arc<AgentState>,
}

#[async_trait::async_trait]
impl mofa_kernel::agent::components::tool::Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn description(&self) -> &str {
        "Stores a key-value pair in the MoFA agent's in-memory store. \
         Values persist for the lifetime of this server process and are \
         accessible by any MCP client via memory_read."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key":   { "type": "string", "description": "Memory key" },
                "value": { "type": "string", "description": "Value to store" }
            },
            "required": ["key", "value"]
        })
    }

    async fn execute(&self, input: ToolInput, _ctx: &AgentContext) -> ToolResult {
        self.state.tick();
        let key = match input.arguments.get("key").and_then(|v| v.as_str()) {
            Some(k) => k.to_string(),
            None => return ToolResult::failure("Missing required argument: 'key'"),
        };
        let value = match input.arguments.get("value").and_then(|v| v.as_str()) {
            Some(v) => v.to_string(),
            None => return ToolResult::failure("Missing required argument: 'value'"),
        };

        let mut mem = self.state.memory.lock().await;
        if let Err(e) = mem.store(&key, MemoryValue::Text(value)).await {
            return ToolResult::failure(format!("Memory write failed: {}", e));
        }

        let stats = mem.stats().await.unwrap_or_default();
        ToolResult::success(serde_json::json!({
            "stored":     key,
            "total_keys": stats.total_items,
        }))
    }
}

// ============================================================================
// Tool: memory_read
// ============================================================================

struct MemoryReadTool {
    state: Arc<AgentState>,
}

#[async_trait::async_trait]
impl mofa_kernel::agent::components::tool::Tool for MemoryReadTool {
    fn name(&self) -> &str {
        "memory_read"
    }

    fn description(&self) -> &str {
        "Reads a value from the MoFA agent's in-memory store by key. \
         Use memory_list to see all available keys."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Memory key to retrieve" }
            },
            "required": ["key"]
        })
    }

    async fn execute(&self, input: ToolInput, _ctx: &AgentContext) -> ToolResult {
        self.state.tick();
        let key = match input.arguments.get("key").and_then(|v| v.as_str()) {
            Some(k) => k,
            None => return ToolResult::failure("Missing required argument: 'key'"),
        };

        let mem = self.state.memory.lock().await;
        match mem.retrieve(key).await {
            Ok(Some(MemoryValue::Text(v))) => {
                ToolResult::success(serde_json::json!({ "key": key, "value": v, "found": true }))
            }
            Ok(Some(other)) => {
                ToolResult::success(
                    serde_json::json!({ "key": key, "value": format!("{:?}", other), "found": true }),
                )
            }
            Ok(None) => {
                ToolResult::success(serde_json::json!({ "key": key, "value": null, "found": false }))
            }
            Err(e) => ToolResult::failure(format!("Memory read failed: {}", e)),
        }
    }
}

// ============================================================================
// Tool: memory_list
// ============================================================================

struct MemoryListTool {
    state: Arc<AgentState>,
}

#[async_trait::async_trait]
impl mofa_kernel::agent::components::tool::Tool for MemoryListTool {
    fn name(&self) -> &str {
        "memory_list"
    }

    fn description(&self) -> &str {
        "Lists all keys currently stored in the MoFA agent's in-memory store, \
         along with memory usage statistics."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _input: ToolInput, _ctx: &AgentContext) -> ToolResult {
        self.state.tick();
        let mem = self.state.memory.lock().await;

        let items = match mem.search("", usize::MAX).await {
            Ok(items) => items,
            Err(e) => return ToolResult::failure(format!("Memory list failed: {}", e)),
        };

        let mut keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        keys.sort_unstable();

        let stats = mem.stats().await.unwrap_or_default();

        ToolResult::success(serde_json::json!({
            "keys":         keys,
            "count":        stats.total_items,
            "memory_bytes": stats.memory_bytes,
        }))
    }
}

// ============================================================================
// Tool: agent_run
// ============================================================================

struct AgentRunTool {
    state: Arc<AgentState>,
}

#[async_trait::async_trait]
impl mofa_kernel::agent::components::tool::Tool for AgentRunTool {
    fn name(&self) -> &str {
        "agent_run"
    }

    fn description(&self) -> &str {
        "Runs the built-in MoFA agent with a task. Supported tasks: \
         'summarize' (first sentence), 'keywords' (top-10 by frequency), \
         'sentiment' (positive/negative/neutral), 'word_count', 'reverse'. \
         The agent automatically persists its output to memory under 'agent:last_output'."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "enum": ["summarize", "keywords", "sentiment", "word_count", "reverse"],
                    "description": "Task for the agent to perform"
                },
                "input": {
                    "type": "string",
                    "description": "Input text for the agent"
                }
            },
            "required": ["task", "input"]
        })
    }

    async fn execute(&self, input: ToolInput, _ctx: &AgentContext) -> ToolResult {
        let invocation = self.state.tick();

        let task = match input.arguments.get("task").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return ToolResult::failure("Missing required argument: 'task'"),
        };
        let text = match input.arguments.get("input").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return ToolResult::failure("Missing required argument: 'input'"),
        };

        // Agent execution: think -> act -> observe
        let output = match task {
            "summarize" => {
                let first = text
                    .split(['.', '!', '?'])
                    .next()
                    .unwrap_or(text)
                    .trim();
                serde_json::json!({
                    "summary":         first,
                    "original_length": text.len(),
                    "summary_length":  first.len(),
                })
            }
            "keywords" => {
                let mut freq: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for word in text.split_whitespace() {
                    let w = word
                        .trim_matches(|c: char| !c.is_alphabetic())
                        .to_lowercase();
                    if w.len() > 4 {
                        *freq.entry(w).or_insert(0) += 1;
                    }
                }
                let mut ranked: Vec<(usize, String)> =
                    freq.into_iter().map(|(k, v)| (v, k)).collect();
                ranked.sort_by(|a, b| b.0.cmp(&a.0));
                let top: Vec<&str> =
                    ranked.iter().take(10).map(|(_, k)| k.as_str()).collect();
                serde_json::json!({ "keywords": top })
            }
            "sentiment" => {
                const POS: &[&str] = &[
                    "good", "great", "excellent", "love", "wonderful",
                    "amazing", "happy", "best", "fantastic", "positive",
                ];
                const NEG: &[&str] = &[
                    "bad", "terrible", "awful", "hate", "horrible",
                    "worst", "sad", "negative", "poor", "dreadful",
                ];
                let lower = text.to_lowercase();
                let pos = POS.iter().filter(|w| lower.contains(*w)).count();
                let neg = NEG.iter().filter(|w| lower.contains(*w)).count();
                let label = if pos > neg {
                    "positive"
                } else if neg > pos {
                    "negative"
                } else {
                    "neutral"
                };
                serde_json::json!({
                    "sentiment":        label,
                    "positive_signals": pos,
                    "negative_signals": neg,
                })
            }
            "word_count" => {
                let words = text.split_whitespace().count();
                let chars = text.chars().count();
                let sentences = text
                    .split(['.', '!', '?'])
                    .filter(|s| !s.trim().is_empty())
                    .count();
                serde_json::json!({
                    "words":     words,
                    "characters": chars,
                    "sentences": sentences,
                })
            }
            "reverse" => {
                serde_json::json!({
                    "reversed": text.chars().rev().collect::<String>()
                })
            }
            unknown => {
                return ToolResult::failure(format!(
                    "Unknown task '{}'. Supported: summarize, keywords, sentiment, word_count, reverse",
                    unknown
                ))
            }
        };

        // Persist result so other tools (or the next session) can read it
        {
            let mut mem = self.state.memory.lock().await;
            let _ = mem
                .store("agent:last_output", MemoryValue::Text(output.to_string()))
                .await;
            let _ = mem
                .store("agent:last_task", MemoryValue::Text(task.to_string()))
                .await;
        }

        ToolResult::success(serde_json::json!({
            "invocation": invocation,
            "task":       task,
            "status":     "completed",
            "output":     output,
            "hint":       "Result saved to agent memory. Use memory_read with key 'agent:last_output'.",
        }))
    }
}

// ============================================================================
// Entry point
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let config = McpHostConfig::new("mofa-agent", "127.0.0.1", port)
        .with_version("0.1.0")
        .with_instructions(
            "A MoFA agent exposed over MCP. Supports persistent in-process memory \
             (memory_write/read/list), agent task execution (agent_run), and \
             health monitoring (health_check). State is shared across all MCP \
             sessions for the lifetime of this process.",
        );

    // Shared state: single memory store + counters across all tools and sessions
    let state = AgentState::new();

    let mut server = McpServerManager::new(config);
    server.register_tool(HealthCheckTool { state: Arc::clone(&state) }.into_dynamic())?;
    server.register_tool(MemoryWriteTool { state: Arc::clone(&state) }.into_dynamic())?;
    server.register_tool(MemoryReadTool  { state: Arc::clone(&state) }.into_dynamic())?;
    server.register_tool(MemoryListTool  { state: Arc::clone(&state) }.into_dynamic())?;
    server.register_tool(AgentRunTool    { state: Arc::clone(&state) }.into_dynamic())?;

    let tool_names = server.registered_tools();
    tracing::info!("{} tools registered:", tool_names.len());
    for name in &tool_names {
        tracing::info!("  - {}", name);
    }
    tracing::info!("MCP endpoint: http://127.0.0.1:{}/mcp", port);

    let ct = CancellationToken::new();
    let ct2 = ct.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("Shutting down...");
            ct2.cancel();
        }
    });

    server.serve_with_cancellation(ct).await?;
    Ok(())
}
