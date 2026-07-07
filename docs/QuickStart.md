# MoFA Quick Start

> Get from zero to a running agent in under 10 minutes.

---

## Prerequisites

- **Rust** stable toolchain (edition 2024 — requires Rust ≥ 1.85)
- **Git**

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

#### Verify

```bash
rustc --version   # 1.85.0 or newer
cargo --version
```

#### Windows

Use the installer from [rustup.rs](https://rustup.rs). Make sure `%USERPROFILE%\.cargo\bin` is on your `PATH`.

#### macOS (Homebrew)

```bash
brew install rustup
rustup-init
```

---

## Get the Source

```bash
git clone https://github.com/mofa-org/mofa.git
cd mofa
```

---

## Building the Project

```bash
# Build the entire workspace
cargo build

# Release build (optimised)
cargo build --release

# Build a single crate
cargo build -p mofa-sdk
```

### Verify everything compiles and tests pass

```bash
cargo check          # fast, no artifacts
cargo test           # full test suite
cargo test -p mofa-sdk   # test the SDK only
```

---

## Setup your IDE

**VS Code** (recommended):

1. Install the [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer) extension.
2. Open the workspace root — `rust-analyzer` picks up `Cargo.toml` automatically.

**JetBrains RustRover / IntelliJ + Rust plugin**: open the folder and let the IDE index the Cargo workspace.

> See [CONTRIBUTING.md](../CONTRIBUTING.md) for architecture rules you should know before editing code.

---

## Setup your LLM Environment

MoFA supports **OpenAI**, **Anthropic**, **Google Gemini**, and any **OpenAI-compatible endpoint** (Ollama, vLLM, OpenRouter, …).

Create a `.env` file in your project root (it is loaded automatically by the `dotenvy` helper used in the examples):

### OpenAI

```env
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-4o           # optional, default: gpt-4o
```

### Anthropic

```env
ANTHROPIC_API_KEY=sk-ant-...
ANTHROPIC_MODEL=claude-sonnet-4-5-latest   # optional
```

### OpenAI-compatible endpoint (Ollama, vLLM, OpenRouter, …)

```env
OPENAI_API_KEY=ollama          # or your key
OPENAI_BASE_URL=http://localhost:11434/v1
OPENAI_MODEL=llama3.2
```

### Google Gemini (via OpenRouter)

```env
OPENAI_API_KEY=<your_openrouter_key>
OPENAI_BASE_URL=https://openrouter.ai/api/v1
OPENAI_MODEL=google/gemini-2.0-flash-001
```

---

## Your First Agent — Step by Step

Add `mofa-sdk` and `tokio` to your `Cargo.toml`:

```toml
[dependencies]
mofa-sdk = { path = "../mofa/crates/mofa-sdk" }   # local path while developing
tokio    = { version = "1", features = ["full"] }
dotenvy  = "0.15"
```

Then write your agent:

```rust
//! Minimal MoFA agent that answers a question with an LLM.

use std::sync::Arc;
use dotenvy::dotenv;
use mofa_sdk::kernel::agent::prelude::*;
use mofa_sdk::llm::{LLMClient, openai_from_env};

struct LLMAgent {
    id: String,
    name: String,
    capabilities: AgentCapabilities,
    state: AgentState,
    client: LLMClient,
}

impl LLMAgent {
    fn new(client: LLMClient) -> Self {
        Self {
            id: "llm-agent-1".to_string(),
            name: "LLM Agent".to_string(),
            capabilities: AgentCapabilities::builder()
                .tag("llm").tag("qa")
                .input_type(InputType::Text)
                .output_type(OutputType::Text)
                .build(),
            state: AgentState::Created,
            client,
        }
    }
}

#[async_trait]
impl MoFAAgent for LLMAgent {
    fn id(&self)           -> &str               { &self.id }
    fn name(&self)         -> &str               { &self.name }
    fn capabilities(&self) -> &AgentCapabilities { &self.capabilities }
    fn state(&self)        -> AgentState         { self.state.clone() }

    async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
        self.state = AgentState::Ready;
        Ok(())
    }

    async fn execute(&mut self, input: AgentInput, _ctx: &AgentContext) -> AgentResult<AgentOutput> {
        self.state = AgentState::Executing;
        let answer = self.client
            .ask_with_system("You are a helpful Rust expert.", &input.to_text())
            .await
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        self.state = AgentState::Ready;
        Ok(AgentOutput::text(answer))
    }

    async fn shutdown(&mut self) -> AgentResult<()> {
        self.state = AgentState::Shutdown;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();   // IMPORTANT: loads .env

    let provider = openai_from_env()?;
    let client   = LLMClient::new(Arc::new(provider));

    let mut agent = LLMAgent::new(client);
    let ctx       = AgentContext::new("exec-001");

    agent.initialize(&ctx).await?;

    let output = agent.execute(
        AgentInput::text("What is the borrow checker in Rust?"),
        &ctx,
    ).await?;

    println!("{}", output.as_text().unwrap_or("(no answer)"));
    agent.shutdown().await?;
    Ok(())
}
```

Run it:

```bash
cargo run
```

---

## Running the Examples

The `examples/` directory contains 27+ ready-to-run demos.

```bash
# Echo / no-LLM baseline
cargo run -p chat_stream

# ReAct agent (reasoning + tool use)
cargo run -p react_agent

# Secretary agent (human-in-the-loop)
cargo run -p secretary_agent

# Multi-agent coordination patterns
cargo run -p multi_agent_coordination

# Rhai hot-reload scripting
cargo run -p rhai_hot_reload

# Adaptive collaboration
cargo run -p adaptive_collaboration_agent
```

> All examples read credentials from environment variables or a local `.env` file.

For a full list see [examples/README.md](../examples/README.md).

---

## Next Steps

| Goal | Where to look |
|---|---|
| Architecture deep-dive | [CLAUDE.md](../CLAUDE.md) |
| API reference | [docs/architecture.md](architecture.md) |
| Add your own LLM provider | Implement `LLMProvider` from `mofa_sdk::llm` |
| Write a Rhai runtime plugin | `examples/rhai_scripting/` |
| Build a WASM plugin | `examples/wasm_plugin/` |
| Contribute a fix or feature | [CONTRIBUTING.md](../CONTRIBUTING.md) |
| Ask a question | [GitHub Discussions](https://github.com/mofa-org/mofa/discussions) · [Discord](https://discord.com/invite/hKJZzDMMm9) (if invite fails, ask in Discussions) |

---

**English** | [简体中文](zh-CN/QuickStart.md)
