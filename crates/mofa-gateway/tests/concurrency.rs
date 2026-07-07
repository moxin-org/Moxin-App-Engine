use mofa_kernel::agent::{AgentCapabilities, AgentContext, AgentInput, AgentOutput, AgentResult, AgentState, MoFAAgent};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

struct SlowAgent {
    id: String,
    name: String,
    capabilities: AgentCapabilities,
    state: AgentState,
}

impl SlowAgent {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: "Slow Agent".to_string(),
            capabilities: AgentCapabilities::default(),
            state: AgentState::Ready,
        }
    }
}

#[async_trait::async_trait]
impl MoFAAgent for SlowAgent {
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
        Ok(())
    }

    async fn execute(
        &self,
        _input: AgentInput,
        _ctx: &AgentContext,
    ) -> AgentResult<AgentOutput> {
        // Sleep to simulate long-running LLM call
        // This ensures the read lock is held for a while
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(AgentOutput::text("done"))
    }

    async fn shutdown(&mut self) -> AgentResult<()> {
        self.state = AgentState::Shutdown;
        Ok(())
    }
}

#[tokio::test]
async fn test_agent_concurrent_read_try_write() {
    let agent_id = "test-agent-123";
    let slow_agent = SlowAgent::new(agent_id);
    let agent_arc: Arc<RwLock<dyn MoFAAgent>> = Arc::new(RwLock::new(slow_agent));

    let agent_clone_for_execute = agent_arc.clone();
    
    // Spawn task to simulate `chat.rs` execution holding a read lock
    let handle = tokio::spawn(async move {
        let agent = agent_clone_for_execute.read().await;
        let ctx = AgentContext::new("test");
        let input = AgentInput::text("hello");
        
        let start = std::time::Instant::now();
        let _ = agent.execute(input, &ctx).await;
        let elapsed = start.elapsed();
        
        // Ensure it actually slept
        assert!(elapsed.as_millis() >= 450);
    });

    // Wait a brief moment to ensure task acquires the read lock
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Simulate `stop_agent` or `delete_agent` which attempts try_write
    let write_attempt = agent_arc.try_write();
    
    // It should fail because the read lock is held!
    assert!(
        write_attempt.is_err(), 
        "try_write should fail because a read lock is held by the executing task"
    );
    
    // Wait for the spawned task to complete
    handle.await.unwrap();

    // After read lock is dropped, try_write should succeed
    let write_attempt_after = agent_arc.try_write();
    assert!(
        write_attempt_after.is_ok(),
        "try_write should succeed after the read lock is dropped"
    );
}
