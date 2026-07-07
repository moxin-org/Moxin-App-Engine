use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use mofa_kernel::agent::{
    AgentMemory, ConflictDetector, ConflictInfo, CoordinationError, CoordinationGovernor,
    CoordinationResult, GovernanceConfig, HandoffContext, HandoffPacket, HandoffProtocol,
    MemoryObject, MemoryRef, ResolutionStrategy,
};

// ─────────────────────────────────────────────────────────────────────────────
// Mock implementations for coordination traits
// ─────────────────────────────────────────────────────────────────────────────

struct MockMemoryStore {
    store: Mutex<HashMap<Uuid, MemoryObject>>,
    handoffs: Mutex<Vec<HandoffPacket>>,
}

impl MockMemoryStore {
    fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
            handoffs: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl AgentMemory for MockMemoryStore {
    async fn write(
        &self,
        content: &str,
        agent_id: &str,
        workflow_id: &str,
    ) -> CoordinationResult<MemoryRef> {
        let memory_id = Uuid::new_v4();
        let obj = MemoryObject::new(
            memory_id,
            agent_id.to_string(),
            content.to_string(),
            workflow_id.to_string(),
            0_u64,
        );

        self.store.lock().unwrap().insert(memory_id, obj);

        Ok(MemoryRef { id: memory_id })
    }

    async fn read(&self, query: &str, limit: usize) -> CoordinationResult<Vec<MemoryObject>> {
        let store = self.store.lock().unwrap();

        let results = store
            .values()
            .filter(|obj| obj.content.contains(query))
            .take(limit)
            .cloned()
            .collect();

        Ok(results)
    }

    async fn delete(&self, memory_ref: &MemoryRef) -> CoordinationResult<()> {
        self.store.lock().unwrap().remove(&memory_ref.id);
        Ok(())
    }

    async fn list_by_workflow(&self, workflow_id: &str) -> CoordinationResult<Vec<MemoryObject>> {
        let store = self.store.lock().unwrap();

        let results = store
            .values()
            .filter(|obj| obj.workflow_id == workflow_id)
            .cloned()
            .collect();

        Ok(results)
    }
}

#[async_trait]
impl HandoffProtocol for MockMemoryStore {
    async fn create_handoff(
        &self,
        from: &str,
        to: &str,
        context: HandoffContext,
    ) -> CoordinationResult<HandoffPacket> {
        // Derive workflow_id from the first memory ref's workflow, or default.
        let workflow_id = {
            let store = self.store.lock().unwrap();
            context
                .memory_refs
                .first()
                .and_then(|r| store.get(&r.id))
                .map(|obj| obj.workflow_id.clone())
                .unwrap_or_default()
        };

        let packet = HandoffPacket::new(
            Uuid::new_v4(),
            from.to_string(),
            to.to_string(),
            context.task_completed,
            context.decisions,
            context.confidence,
            context.memory_refs,
            context.next_task,
            workflow_id,
            // Demo: use a fixed timestamp; production impls should use a real clock.
            0_u64,
        );

        self.handoffs.lock().unwrap().push(packet.clone());
        Ok(packet)
    }

    async fn receive_handoff(
        &self,
        agent_id: &str,
    ) -> CoordinationResult<Option<HandoffPacket>> {
        let handoffs = self.handoffs.lock().unwrap();
        if let Some(pos) = handoffs.iter().position(|h| h.to_agent == agent_id) {
            Ok(Some(handoffs[pos].clone()))
        } else {
            Ok(None)
        }
    }

    async fn acknowledge_handoff(&self, handoff_id: Uuid) -> CoordinationResult<()> {
        let mut handoffs = self.handoffs.lock().unwrap();
        if let Some(pos) = handoffs.iter().position(|h| h.handoff_id == handoff_id) {
            handoffs.remove(pos);
        }
        Ok(())
    }

    async fn list_handoffs(&self, workflow_id: &str) -> CoordinationResult<Vec<HandoffPacket>> {
        let handoffs = self.handoffs.lock().unwrap();
        Ok(handoffs
            .iter()
            .filter(|h| h.workflow_id == workflow_id)
            .cloned()
            .collect())
    }
}

struct MockConflictDetector;

#[async_trait]
impl ConflictDetector for MockConflictDetector {
    fn detect(&self, existing: &MemoryObject, incoming: &MemoryObject) -> Option<ConflictInfo> {
        if existing.content == incoming.content {
            return None;
        }

        Some(ConflictInfo::new(
            Uuid::new_v4(),
            MemoryRef { id: existing.memory_id },
            existing.content.clone(),
            incoming.content.clone(),
            0_u64,
            existing.workflow_id.clone(),
        ))
    }

    async fn resolve(
        &self,
        conflict: &ConflictInfo,
        strategy: ResolutionStrategy,
    ) -> CoordinationResult<MemoryObject> {
        let content = match strategy {
            ResolutionStrategy::KeepExisting => conflict.existing_value.clone(),
            ResolutionStrategy::KeepIncoming => conflict.incoming_value.clone(),
            ResolutionStrategy::Merge => {
                format!("{} | {}", conflict.existing_value, conflict.incoming_value)
            }
            ResolutionStrategy::Escalate => {
                return Err(CoordinationError::Conflict(
                    "Escalation required (mock implementation)".to_string(),
                ))
            }
            // Fallback: default to KeepIncoming for any future ResolutionStrategy
            // variants added under #[non_exhaustive].
            _ => conflict.incoming_value.clone(),
        };

        Ok(MemoryObject::new(
            conflict.memory_ref.id,
            "resolved".to_string(),
            content,
            conflict.workflow_id.clone(),
            0_u64,
        ))
    }

    async fn list_conflicts(&self, _workflow_id: &str) -> CoordinationResult<Vec<ConflictInfo>> {
        Ok(Vec::new())
    }
}

struct MockGovernor {
    audit: Mutex<Vec<HandoffPacket>>,
}

impl MockGovernor {
    fn new() -> Self {
        Self {
            audit: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl CoordinationGovernor for MockGovernor {
    fn check_spawn_allowed(&self, _agent_id: &str, depth: u32, config: &GovernanceConfig) -> bool {
        depth < config.max_spawn_depth
    }

    async fn record_handoff(&self, packet: &HandoffPacket) -> CoordinationResult<()> {
        self.audit.lock().unwrap().push(packet.clone());
        Ok(())
    }

    async fn get_audit_trail(
        &self,
        _workflow_id: &str,
    ) -> CoordinationResult<Vec<HandoffPacket>> {
        Ok(self.audit.lock().unwrap().clone())
    }

    async fn add_to_dead_letter(
        &self,
        failed: HandoffPacket,
        reason: &str,
    ) -> CoordinationResult<()> {
        println!("Dead letter: {} — {}", failed.handoff_id, reason);
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Demo entrypoint
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let store = MockMemoryStore::new();
    let governor = MockGovernor::new();
    let detector = MockConflictDetector;
    let config = GovernanceConfig::default();

    // ── Step 1: Agent A writes to shared memory ─────────────────────────────
    let ref_a = store
        .write(
            "BTC showing bullish divergence",
            "agent_a",
            "wf_001",
        )
        .await?;
    println!("Agent A wrote memory: {:?}", ref_a);

    // ── Step 2: Agent B reads shared memory ─────────────────────────────────
    let memories = store.read("bullish", 5).await?;
    println!("Agent B found {} memories", memories.len());

    // ── Step 3: Parallel write — conflict check ─────────────────────────────
    let _ref_b = store
        .write(
            "BTC showing bearish reversal",
            "agent_b",
            "wf_001",
        )
        .await?;

    let mut bullish = store.read("bullish", 1).await?;
    let mut bearish = store.read("bearish", 1).await?;

    if let (Some(obj_a), Some(obj_b)) = (bullish.pop(), bearish.pop())
        && let Some(conflict) = detector.detect(&obj_a, &obj_b)
    {
        println!(
            "Conflict detected: {} vs {}",
            conflict.existing_value, conflict.incoming_value
        );
        let resolved = detector
            .resolve(&conflict, ResolutionStrategy::KeepIncoming)
            .await?;
        println!("Resolved to: {}", resolved.content);
    }

    // ── Step 4: Agent A creates handoff for Agent B ─────────────────────────
    let context = HandoffContext::new(
        "Market analysis complete".into(),
        vec!["BTC analysis done".into()],
        0.87,
        "Generate summary report".into(),
        vec![ref_a],
    );

    let packet = store
        .create_handoff("agent_a", "agent_b", context)
        .await?;
    println!("Handoff created: {}", packet.handoff_id);

    // ── Step 5: Governor records + checks spawn ─────────────────────────────
    governor.record_handoff(&packet).await?;
    let allowed = governor.check_spawn_allowed("agent_b", 1, &config);
    println!("Spawn allowed: {}", allowed);

    // ── Step 6: Agent B receives handoff ────────────────────────────────────
    if let Some(h) = store.receive_handoff("agent_b").await? {
        println!(
            "Agent B received: {} (confidence: {})",
            h.task_completed, h.confidence
        );
        store.acknowledge_handoff(h.handoff_id).await?;
    }

    // ── Step 7: Audit trail ─────────────────────────────────────────────────
    let trail = governor.get_audit_trail("wf_001").await?;
    println!("Audit trail: {} handoffs recorded", trail.len());

    println!("\nCoordination demo complete — all 4 traits working.");

    Ok(())
}

