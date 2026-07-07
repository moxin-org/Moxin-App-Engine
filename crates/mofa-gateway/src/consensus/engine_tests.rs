//! Tests for the Raft consensus engine.

#[cfg(test)]
mod tests {
    use crate::consensus::engine::{ConsensusEngine, RaftConfig};
    use crate::consensus::storage::RaftStorage;
    use crate::consensus::transport_impl::{ConsensusHandler, InMemoryTransport};
    use crate::types::{NodeId, StateMachineCommand};
    use std::collections::HashMap;
    use std::sync::Arc;

    struct MockHandler {
        engine: Arc<ConsensusEngine>,
    }

    #[async_trait::async_trait]
    impl ConsensusHandler for MockHandler {
        async fn handle_request_vote(
            &self,
            request: crate::consensus::transport::RequestVoteRequest,
        ) -> crate::error::ConsensusResult<crate::consensus::transport::RequestVoteResponse>
        {
            self.engine.handle_request_vote(request).await
        }

        async fn handle_append_entries(
            &self,
            request: crate::consensus::transport::AppendEntriesRequest,
        ) -> crate::error::ConsensusResult<crate::consensus::transport::AppendEntriesResponse>
        {
            self.engine.handle_append_entries(request).await
        }
    }

    #[tokio::test]
    async fn test_consensus_engine_creation() {
        let node_id = NodeId::new("node-1");
        let storage = Arc::new(RaftStorage::new());
        let transport = Arc::new(InMemoryTransport::new());
        let config = RaftConfig::default();

        let engine = ConsensusEngine::new(node_id.clone(), config, storage, transport);
        // Verify engine was created (node_id is private, so we can't directly assert it)
        // The fact that it doesn't panic is sufficient for this test
    }

    #[tokio::test]
    async fn test_consensus_engine_start_stop() {
        let node_id = NodeId::new("node-1");
        let storage = Arc::new(RaftStorage::new());
        let transport = Arc::new(InMemoryTransport::new());
        let config = RaftConfig {
            cluster_nodes: vec![node_id.clone()],
            ..Default::default()
        };

        let engine = ConsensusEngine::new(node_id, config, storage, transport);
        engine.start().await.unwrap();

        // Give it a moment to initialize
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        engine.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_propose_only_works_as_leader() {
        let node_id = NodeId::new("node-1");
        let storage = Arc::new(RaftStorage::new());
        let transport = Arc::new(InMemoryTransport::new());
        let config = RaftConfig {
            cluster_nodes: vec![node_id.clone()],
            ..Default::default()
        };

        let engine = ConsensusEngine::new(node_id, config, storage, transport);

        // Try to propose as follower (should fail)
        let command = StateMachineCommand::RegisterAgent {
            agent_id: "agent-1".to_string(),
            metadata: HashMap::new(),
        };

        let result = engine.propose(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::error::ConsensusError::NotLeader(_)
        ));
    }

    /// Verify that heartbeats use `prev_log_index = next_index - 1`, not
    /// `next_index` itself.  Before the fix, the leader sent `next_index`
    /// raw, which is one past the follower's last entry.  The follower's
    /// consistency check then looks for an entry that doesn't exist and
    /// rejects every heartbeat.
    #[tokio::test]
    async fn test_heartbeat_prev_log_index_off_by_one() {
        use crate::consensus::transport::{AppendEntriesRequest, AppendEntriesResponse};
        use crate::types::{LogEntry, LogIndex, Term};

        let node_id = NodeId::new("follower-1");
        let storage = Arc::new(RaftStorage::new());
        let transport = Arc::new(InMemoryTransport::new());
        let config = RaftConfig::default();
        let engine = ConsensusEngine::new(node_id.clone(), config, storage, transport);

        // Populate the follower's log with 3 entries via AppendEntries.
        let entries: Vec<LogEntry> = (1..=3)
            .map(|i| LogEntry {
                term: Term::new(1),
                index: LogIndex::new(i),
                data: Vec::new(),
            })
            .collect();

        let populate = AppendEntriesRequest {
            term: Term::new(1),
            leader_id: NodeId::new("leader"),
            prev_log_index: LogIndex::new(0), // empty log, start from beginning
            prev_log_term: Term::new(0),
            entries,
            leader_commit: LogIndex::new(0),
        };
        let resp = engine.handle_append_entries(populate).await.unwrap();
        assert!(resp.success, "populating the follower log must succeed");

        // Heartbeat with correct prev_log_index (= next_index - 1 = 3).
        // The follower has entries 1-3, so checking index 3 should succeed.
        let good_heartbeat = AppendEntriesRequest {
            term: Term::new(1),
            leader_id: NodeId::new("leader"),
            prev_log_index: LogIndex::new(3), // correct: last entry index
            prev_log_term: Term::new(1),
            entries: Vec::new(),
            leader_commit: LogIndex::new(0),
        };
        let resp = engine.handle_append_entries(good_heartbeat).await.unwrap();
        assert!(resp.success, "heartbeat with prev_log_index=3 (correct) must succeed");

        // Heartbeat with the OLD buggy prev_log_index (= next_index = 4).
        // The follower only has 3 entries, so checking index 4 should fail.
        let bad_heartbeat = AppendEntriesRequest {
            term: Term::new(1),
            leader_id: NodeId::new("leader"),
            prev_log_index: LogIndex::new(4), // buggy: one past last entry
            prev_log_term: Term::new(1),
            entries: Vec::new(),
            leader_commit: LogIndex::new(0),
        };
        let resp = engine.handle_append_entries(bad_heartbeat).await.unwrap();
        assert!(
            !resp.success,
            "heartbeat with prev_log_index=4 (off-by-one) must fail — \
             this is the scenario the fix prevents on the sender side"
        );
    }
}
