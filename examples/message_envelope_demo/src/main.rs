//! # MessageEnvelope — Real-World Framework Integration
//!
//! This example demonstrates how [`MessageEnvelope`] integrates with the core
//! MoFA framework to address real-world concerns:
//!
//! - **[`MoFAAgent`] trait** — Both `ProducerAgent` and `ConsumerAgent` are
//!   full agent implementations with canonical lifecycle methods.
//! - **[`AgentBus`]** — The envelope is physically transmitted between agents
//!   via the framework's built-in inter-agent communication bus
//!   ([`CommunicationMode::PointToPoint`]).
//! - **Trace propagation** — `trace_id` is stamped at the producing side and
//!   echoed by the consumer in every log line, enabling end-to-end
//!   distributed-tracing correlation without any extra infrastructure.
//! - **Protocol-version guard** — `check_version()` is called *inside*
//!   `ConsumerAgent::execute()` before touching the payload, which is the
//!   correct place for this guard in framework code.
//! - **Backward / forward compatibility** — The same consumer code correctly
//!   handles legacy payloads (no `version` field) and raises a structured
//!   [`AgentError::ProtocolVersionMismatch`] for future protocol versions,
//!   without any special-casing on the send side.
//!
//! ## Pipeline
//!
//! ```text
//!  ProducerAgent                                ConsumerAgent
//!  ─────────────                                ─────────────
//!  ChatPayload                                  MessageEnvelope<ChatPayload>
//!  └─ MessageEnvelope::new(payload)             └─ check_version()     ← ProtocolVersionMismatch
//!       .with_trace_id("trace-…")               └─ process payload     ← AgentOutput::text(…)
//!  └─ serde_json::to_string(envelope)
//!  └─ AgentBus::send_message(PointToPoint)
//!                   │
//!             AgentBus (bincode wire)
//!                   │
//!             AgentBus::receive_message(PointToPoint)
//! ```
//!
//! Run with:
//! ```bash
//! # from the examples workspace root:
//! cargo run -p message_envelope_demo
//! ```

use async_trait::async_trait;
use mofa_kernel::{
    AgentBus, CommunicationMode,
    agent::{
        AgentCapabilities, AgentContext, AgentError, AgentResult, AgentState,
        MoFAAgent,
        traits::AgentMetadata,
        types::{AgentInput, AgentOutput},
    },
    llm::{MessageEnvelope, ProtocolVersion},
    message::AgentMessage,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

// ============================================================================
// Shared payload type
// ============================================================================

/// A simple chat payload that travels inside a [`MessageEnvelope`].
///
/// In production this would live in a shared crate consumed by both the
/// producer and consumer.  Here it is defined once and used by both agents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ChatPayload {
    user: String,
    message: String,
}

// ============================================================================
// ProducerAgent
// ============================================================================

/// An agent that wraps outgoing messages in a [`MessageEnvelope`] and
/// forwards them over the [`AgentBus`].
///
/// The key responsibilities are:
/// 1. Stamp a `trace_id` onto every outgoing envelope.
/// 2. Serialize the envelope to JSON (the wire format expected by the consumer).
/// 3. Deliver the bytes to the bus using [`CommunicationMode::PointToPoint`].
struct ProducerAgent {
    id: String,
    name: String,
    capabilities: AgentCapabilities,
    state: AgentState,
    /// Reference to the shared bus — injected at construction time.
    bus: AgentBus,
    /// ID of the downstream consumer agent.
    consumer_id: String,
}

const PRODUCER_ID: &str = "producer-agent";
const CONSUMER_ID: &str = "consumer-agent";

impl ProducerAgent {
    fn new(bus: AgentBus, consumer_id: impl Into<String>) -> Self {
        Self {
            id: PRODUCER_ID.to_string(),
            name: "Producer Agent".to_string(),
            capabilities: AgentCapabilities::builder()
                .tag("envelope-producer")
                .build(),
            state: AgentState::Created,
            bus,
            consumer_id: consumer_id.into(),
        }
    }
}

#[async_trait]
impl MoFAAgent for ProducerAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> &AgentCapabilities {
        &self.capabilities
    }

    fn state(&self) -> AgentState {
        self.state.clone()
    }

    async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
        self.state = AgentState::Ready;
        info!("[ProducerAgent] Initialized.");
        Ok(())
    }

    /// Wraps `input` in a [`MessageEnvelope`] and sends it to the consumer.
    ///
    /// `input` is expected to be `AgentInput::Json` whose value deserializes
    /// into a [`ChatPayload`].  The `trace_id` is freshly generated here; in a
    /// real system it would be propagated from the incoming request context.
    async fn execute(
        &mut self,
        input: AgentInput,
        _ctx: &AgentContext,
    ) -> AgentResult<AgentOutput> {
        self.state = AgentState::Executing;

        // --- Deserialize the raw input into our domain payload ---------------
        let payload: ChatPayload = serde_json::from_value(input.to_json())
            .map_err(|e| AgentError::ExecutionFailed(format!("Invalid ChatPayload: {e}")))?;

        // --- Wrap in a protocol-versioned envelope with a fresh trace ID -----
        let trace_id = Uuid::new_v4().to_string();
        let envelope = MessageEnvelope::new(payload).with_trace_id(&trace_id);

        info!(
            "[ProducerAgent] Sending envelope (version={}) to '{}' [trace_id={}]",
            envelope.version,
            self.consumer_id,
            trace_id,
        );

        // --- Serialize the envelope to JSON bytes used as the task content ---
        let json = serde_json::to_string(&envelope)
            .map_err(|e| AgentError::ExecutionFailed(format!("Envelope serialization: {e}")))?;

        // --- Forward via the AgentBus ----------------------------------------
        // The consumer registered a PointToPoint channel keyed on *our* ID
        // (`PRODUCER_ID`).  `send_message` requires `sender_id` for lookup.
        let bus_msg = AgentMessage::TaskRequest {
            task_id: trace_id.clone(),
            content: json,
        };

        self.bus
            .send_message(
                PRODUCER_ID,                                                    // sender_id
                CommunicationMode::PointToPoint(self.consumer_id.clone()),      // delivery mode
                &bus_msg,
            )
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("Bus delivery failed: {e}")))?;

        info!("[ProducerAgent] Envelope dispatched [trace_id={}].", trace_id);
        self.state = AgentState::Ready;
        Ok(AgentOutput::text(format!(
            "Envelope dispatched with trace_id={trace_id}"
        )))
    }

    async fn shutdown(&mut self) -> AgentResult<()> {
        self.state = AgentState::Shutdown;
        info!("[ProducerAgent] Shutdown.");
        Ok(())
    }
}

// ============================================================================
// ConsumerAgent
// ============================================================================

/// An agent that receives raw [`AgentMessage`]s from the [`AgentBus`],
/// decodes the embedded [`MessageEnvelope`], enforces the version guard,
/// and processes the typed payload.
///
/// This is where the framework integration is most visible:
/// - Version gating happens *before* any business logic.
/// - The `trace_id` is surfaced in every log span so observability tools can
///   correlate the full request chain automatically.
struct ConsumerAgent {
    id: String,
    name: String,
    capabilities: AgentCapabilities,
    state: AgentState,
    bus: AgentBus,
}

impl ConsumerAgent {
    fn new(bus: AgentBus) -> Self {
        Self {
            id: CONSUMER_ID.to_string(),
            name: "Consumer Agent".to_string(),
            capabilities: AgentCapabilities::builder()
                .tag("envelope-consumer")
                .build(),
            state: AgentState::Created,
            bus,
        }
    }

    /// Block until one [`AgentMessage::TaskRequest`] arrives on the bus,
    /// then decode the embedded [`MessageEnvelope`] and return an
    /// [`AgentOutput`].
    ///
    /// In production this would be driven by a runtime event loop; here it is
    /// called directly to keep the demo self-contained.
    async fn receive_and_process(&mut self) -> AgentResult<AgentOutput> {
        // --- Pull the next message off our PointToPoint channel --------------
        // The channel was registered in `initialize()` keyed on the sender ID.
        let raw = self
            .bus
            .receive_message(
                CONSUMER_ID,
                CommunicationMode::PointToPoint(PRODUCER_ID.to_string()),
            )
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("Bus receive failed: {e}")))?;

        let Some(AgentMessage::TaskRequest { task_id, content }) = raw else {
            return Err(AgentError::ExecutionFailed(
                "Unexpected or empty message from bus.".to_string(),
            ));
        };

        info!(
            "[ConsumerAgent] Raw TaskRequest received from bus [task_id={}].",
            task_id
        );

        // --- Decode the JSON body as a MessageEnvelope ----------------------
        let envelope: MessageEnvelope<ChatPayload> = serde_json::from_str(&content)
            .map_err(|e| AgentError::ExecutionFailed(format!("Envelope deser failed: {e}")))?;

        info!(
            "[ConsumerAgent] Envelope decoded [version={}, trace_id={}].",
            envelope.version,
            envelope.trace_id.as_deref().unwrap_or("<none>"),
        );

        // --- Protocol-version guard — MUST happen before reading payload -----
        envelope.check_version()?;

        // --- Business logic --------------------------------------------------
        let trace = envelope.trace_id.as_deref().unwrap_or("<none>");
        let payload = &envelope.payload;

        info!(
            "[ConsumerAgent] Processing message from user '{}' [trace_id={}].",
            payload.user, trace,
        );

        let response = format!(
            "Processed message from '{}': \"{}\" (trace_id={})",
            payload.user, payload.message, trace,
        );

        Ok(AgentOutput::text(response))
    }

    /// Re-processes a raw JSON string that may contain any protocol version.
    ///
    /// In a real gateway this method would be called for every inbound message,
    /// dispatching to the appropriate handler or raising a version-mismatch
    /// alert without crashing the process.
    fn process_raw_json(&self, json: &str, label: &str) {
        match serde_json::from_str::<MessageEnvelope<ChatPayload>>(json) {
            Ok(envelope) => match envelope.check_version() {
                Ok(()) => {
                    let trace = envelope.trace_id.as_deref().unwrap_or("<none>");
                    info!(
                        "[ConsumerAgent] [{label}] OK version={} trace_id={} | user='{}', message='{}'",
                        envelope.version,
                        trace,
                        envelope.payload.user,
                        envelope.payload.message,
                    );
                }
                Err(e) => {
                    warn!(
                        "[ConsumerAgent] [{label}] Version mismatch — refusing payload. Error: {e}"
                    );
                }
            },
            Err(e) => {
                error!("[ConsumerAgent] [{label}] Failed to decode envelope: {e}");
            }
        }
    }
}

#[async_trait]
impl MoFAAgent for ConsumerAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> &AgentCapabilities {
        &self.capabilities
    }

    fn state(&self) -> AgentState {
        self.state.clone()
    }

    async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
        // Register a PointToPoint channel so the producer can deliver messages.
        // The channel key is the *sender's* ID (PRODUCER_ID).
        let meta = AgentMetadata {
            id: self.id.clone(),
            name: self.name.clone(),
            description: None,
            version: None,
            capabilities: self.capabilities.clone(),
            state: AgentState::Ready,
        };
        self.bus
            .register_channel(
                &meta,
                CommunicationMode::PointToPoint(PRODUCER_ID.to_string()),
            )
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("Channel registration: {e}")))?;

        self.state = AgentState::Ready;
        info!("[ConsumerAgent] Initialized and PointToPoint channel registered.");
        Ok(())
    }

    /// Thin facade over `receive_and_process`.
    ///
    /// In a full runtime this would be invoked in a loop by the scheduler;
    /// here we also expose a direct `receive_and_process()` entry-point for
    /// use in `tokio::join!` so both agents can run concurrently.
    async fn execute(
        &mut self,
        _input: AgentInput,
        _ctx: &AgentContext,
    ) -> AgentResult<AgentOutput> {
        self.receive_and_process().await
    }

    async fn shutdown(&mut self) -> AgentResult<()> {
        self.state = AgentState::Shutdown;
        info!("[ConsumerAgent] Shutdown.");
        Ok(())
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("message_envelope_demo=info".parse().unwrap()),
        )
        .init();

    info!("=== MessageEnvelope — Framework Integration Demo ===\n");

    // ── Shared bus ────────────────────────────────────────────────────────────
    let bus = AgentBus::new();

    // ── Instantiate agents ───────────────────────────────────────────────────
    let mut producer = ProducerAgent::new(bus.clone(), CONSUMER_ID);
    let mut consumer = ConsumerAgent::new(bus.clone());

    // ── Shared execution context (minimal; no external services needed) ──────
    let ctx = AgentContext::new("demo-session");

    // ── Lifecycle: initialize ────────────────────────────────────────────────
    producer.initialize(&ctx).await?;
    consumer.initialize(&ctx).await?;

    // =========================================================================
    // Scenario 1 — Happy path: V1 envelope over the AgentBus
    //
    // The producer wraps a ChatPayload in a MessageEnvelope, attaches a fresh
    // trace_id, and sends it over the bus.  The consumer decodes the envelope,
    // calls check_version(), and processes the payload — all inside their
    // respective execute() / receive_and_process() methods.
    //
    // Sequencing note: AgentBus uses broadcast channels internally.  The
    // consumer *must* subscribe before the producer sends; otherwise the
    // broadcast message is missed.  We achieve this by spawning the receive
    // task first, then yielding once so the spawned future reaches its
    // subscribe point before we call the producer.
    // =========================================================================
    info!("\n--- Scenario 1: V1 Envelope — PointToPoint over AgentBus ---");

    let payload = serde_json::json!({ "user": "Alice", "message": "Hello MoFA!" });

    // Spawn the consumer receive loop first so it subscribes to the channel.
    let recv_task = tokio::spawn({
        let bus2 = bus.clone();
        async move {
            let mut c = ConsumerAgent::new(bus2);
            c.state = AgentState::Ready;
            c.receive_and_process().await
        }
    });

    // Yield to allow the spawned task to reach `recv().await`.
    tokio::task::yield_now().await;

    let send_result = producer.execute(AgentInput::json(payload), &ctx).await?;
    let recv_result = recv_task.await.expect("consumer task panicked")?;

    info!("Producer output : {:?}", send_result.as_text());
    info!("Consumer output : {:?}", recv_result.as_text());

    // =========================================================================
    // Scenario 2 — Backward compatibility: legacy JSON (no "version" field)
    //
    // Simulates receiving a message from an older MoFA node built before
    // MessageEnvelope was introduced.  The missing `version` field defaults
    // to V1 via `#[serde(default)]` — no migration step required.
    // =========================================================================
    info!("\n--- Scenario 2: Backward Compatibility (legacy payload, no version field) ---");

    let legacy_json = r#"{
        "payload": {
            "user": "Bob",
            "message": "I'm from an old MoFA node!"
        }
    }"#;

    consumer.process_raw_json(legacy_json, "legacy");

    // =========================================================================
    // Scenario 3 — Forward compatibility guard: future protocol version
    //
    // A future MoFA node tagged its message as version "2".  Our build does
    // not understand V2, so check_version() returns
    // AgentError::ProtocolVersionMismatch before we ever touch the payload.
    // The consumer logs a warning and continues — it does NOT panic.
    // =========================================================================
    info!("\n--- Scenario 3: Future Protocol Version — Graceful Rejection ---");

    let future_json = r#"{
        "version": "2",
        "trace_id": "trace-future-99999",
        "payload": {
            "user": "Eve",
            "message": "I'm from the future!"
        }
    }"#;

    consumer.process_raw_json(future_json, "future-v2");

    // =========================================================================
    // Scenario 4 — Payload mapping: transform the envelope payload type
    //
    // `MessageEnvelope::map` lets you transform the inner payload type while
    // preserving all envelope metadata (version + trace_id).  This is useful
    // when a pipeline stage needs to normalize or enrich the payload without
    // losing traceability.
    // =========================================================================
    info!("\n--- Scenario 4: Payload Mapping (envelope::map) ---");

    let original = MessageEnvelope::new(ChatPayload {
        user: "Charlie".to_string(),
        message: "Transform me!".to_string(),
    })
    .with_trace_id("trace-map-demo");

    // Convert the ChatPayload into a plain summary string, keeping the same
    // version and trace_id so downstream stages can still correlate.
    let summary_envelope = original.map(|p| format!("[{}] {}", p.user, p.message));

    assert_eq!(summary_envelope.version, ProtocolVersion::V1);
    assert_eq!(
        summary_envelope.trace_id.as_deref(),
        Some("trace-map-demo")
    );

    info!(
        "[ConsumerAgent] [map] version={} trace_id={} summary=\"{}\"",
        summary_envelope.version,
        summary_envelope.trace_id.as_deref().unwrap_or("<none>"),
        summary_envelope.payload,
    );

    // ── Lifecycle: shutdown ──────────────────────────────────────────────────
    producer.shutdown().await?;
    consumer.shutdown().await?;

    info!("\n=== Demo complete. ===");
    Ok(())
}
