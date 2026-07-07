use anyhow::Result;
use async_trait::async_trait;
use mofa_sdk::kernel::{AgentError, AgentResult};
use mofa_sdk::llm::{openai_from_env, LLMAgent, LLMAgentBuilder};
use mofa_sdk::workflow::{
    Command, CompiledGraph, GraphState, JsonState, NodeFunc, RuntimeContext, END, START,
    StateGraph, StateGraphImpl,
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// Node 1: initialize session/workflow context.
struct StartNode;

#[async_trait]
impl NodeFunc<JsonState> for StartNode {
    async fn call(&self, _state: &mut JsonState, _ctx: &RuntimeContext) -> AgentResult<Command> {
        Ok(Command::new()
            .update("phase", json!("started"))
            .update("llm_context_cleared", json!(false))
            .continue_())
    }

    fn name(&self) -> &str {
        "start"
    }
}

/// Node 2: simulate an LLM generation step.
struct LlmGenerateNode;

#[async_trait]
impl NodeFunc<JsonState> for LlmGenerateNode {
    async fn call(&self, _state: &mut JsonState, _ctx: &RuntimeContext) -> AgentResult<Command> {
        println!("[llm_generate] Simulating token generation...");
        sleep(Duration::from_millis(250)).await;

        Ok(Command::new()
            .update("phase", json!("generating"))
            .update("generated_text", json!("This is a long assistant answer being spoken..."))
            .continue_())
    }

    fn name(&self) -> &str {
        "llm_generate"
    }
}

/// Node 3: simulate TTS playback, but preempt immediately on barge-in signal.
struct TtsPlaybackNode {
    barge_in_flag: Arc<AtomicBool>,
}

#[async_trait]
impl NodeFunc<JsonState> for TtsPlaybackNode {
    async fn call(&self, _state: &mut JsonState, _ctx: &RuntimeContext) -> AgentResult<Command> {
        println!("[tts_playback] Playing TTS chunks...");

        for idx in 0..30 {
            sleep(Duration::from_millis(100)).await;

            if self.barge_in_flag.load(Ordering::Relaxed) {
                println!("[tts_playback] Barge-in detected at chunk {idx} -> preempt transition");
                return Ok(Command::new()
                    .update("phase", json!("barge_in_detected"))
                    .update("barge_in", json!(true))
                    .goto("barge_in_handler"));
            }
        }

        println!("[tts_playback] Completed without interruption");
        Ok(Command::new()
            .update("phase", json!("tts_completed"))
            .update("barge_in", json!(false))
            .goto("finalize"))
    }

    fn name(&self) -> &str {
        "tts_playback"
    }
}

/// Node 4: preemptive handler, interrupts TTS and clears LLM context.
struct BargeInHandlerNode {
    agent: Option<Arc<LLMAgent>>,
}

#[async_trait]
impl NodeFunc<JsonState> for BargeInHandlerNode {
    async fn call(&self, _state: &mut JsonState, _ctx: &RuntimeContext) -> AgentResult<Command> {
        println!("[barge_in_handler] Preempting output: interrupt TTS + clear context");

        let (interrupted, cleared) = if let Some(agent) = &self.agent {
            agent
                .interrupt_tts()
                .await
                .map_err(|e| AgentError::Other(format!("interrupt_tts failed: {e}")))?;

            let sid = agent.current_session_id().await;
            agent
                .clear_session_history(&sid)
                .await
                .map_err(|e| AgentError::Other(format!("clear_session_history failed: {e}")))?;
            (true, true)
        } else {
            println!("[barge_in_handler] OPENAI_API_KEY missing, running mock cleanup path");
            (true, true)
        };

        Ok(Command::new()
            .update("phase", json!("preempted"))
            .update("tts_interrupted", json!(interrupted))
            .update("llm_context_cleared", json!(cleared))
            .goto("finalize"))
    }

    fn name(&self) -> &str {
        "barge_in_handler"
    }
}

/// Node 5: end state summary.
struct FinalizeNode;

#[async_trait]
impl NodeFunc<JsonState> for FinalizeNode {
    async fn call(&self, state: &mut JsonState, _ctx: &RuntimeContext) -> AgentResult<Command> {
        let phase: Option<String> = state.get_value("phase");
        let barge_in: Option<bool> = state.get_value("barge_in");
        let interrupted: Option<bool> = state.get_value("tts_interrupted");
        let cleared: Option<bool> = state.get_value("llm_context_cleared");

        println!("[finalize] phase={phase:?}, barge_in={barge_in:?}, tts_interrupted={interrupted:?}, llm_context_cleared={cleared:?}");

        Ok(Command::new().return_())
    }

    fn name(&self) -> &str {
        "finalize"
    }
}

fn maybe_build_agent() -> Option<Arc<LLMAgent>> {
    if std::env::var("OPENAI_API_KEY").is_err() {
        return None;
    }

    let provider = openai_from_env().ok()?;
    let agent = LLMAgentBuilder::new()
        .with_name("barge-in-poc-agent")
        .with_provider(Arc::new(provider))
        .with_system_prompt("You are a voice assistant.")
        .build();

    Some(Arc::new(agent))
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== POC C: StateGraph Barge-in Logic ===");
    println!("Demonstrates preemptive transition on audio signal and TTS/context cleanup.\n");

    let barge_in_flag = Arc::new(AtomicBool::new(false));

    let trigger_delay_ms = std::env::var("BARGE_IN_AFTER_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(700);

    let detector_flag = barge_in_flag.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(trigger_delay_ms)).await;
        detector_flag.store(true, Ordering::Relaxed);
        println!("[audio_layer] Voice activity detected -> send BARGE_IN signal");
    });

    let agent = maybe_build_agent();

    let mut graph = StateGraphImpl::<JsonState>::new("barge_in_stategraph_poc");

    graph
        .add_node("start", Box::new(StartNode))
        .add_node("llm_generate", Box::new(LlmGenerateNode))
        .add_node(
            "tts_playback",
            Box::new(TtsPlaybackNode {
                barge_in_flag: barge_in_flag.clone(),
            }),
        )
        .add_node(
            "barge_in_handler",
            Box::new(BargeInHandlerNode {
                agent: agent.clone(),
            }),
        )
        .add_node("finalize", Box::new(FinalizeNode))
        .add_edge(START, "start")
        .add_edge("start", "llm_generate")
        .add_edge("llm_generate", "tts_playback")
        .add_edge("tts_playback", "finalize")
        .add_edge("tts_playback", "barge_in_handler") // preemptive barge-in path
        .add_edge("barge_in_handler", "finalize")
        .add_edge("finalize", END);

    let compiled = graph.compile()?;

    let state = JsonState::new();
    let final_state = compiled.invoke(state, None).await?;

    println!("\nFinal state JSON:");
    println!("{}", serde_json::to_string_pretty(&final_state.to_json()?)?);

    println!("\nPOC complete.");
    println!("- If barge-in triggers during TTS node, graph jumps to barge_in_handler immediately.");
    println!("- Handler interrupts TTS and clears LLM session context (or mock path if no API key).");

    Ok(())
}
