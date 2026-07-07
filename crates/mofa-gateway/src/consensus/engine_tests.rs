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

    /// After a leader change, a follower may carry stale entries from the
    /// old leader that were never committed.  When the new leader replicates
    /// its own entries, `handle_append_entries` must truncate the follower's
    /// log at the first conflict point (Raft §5.3).  Before the fix the
    /// code replaced conflicting entries in-place but left stale entries
    /// beyond the new batch untouched.
    #[tokio::test]
    async fn test_handle_append_entries_truncates_conflicting_tail() {
        use crate::consensus::transport::AppendEntriesRequest;
        use crate::types::{LogEntry, LogIndex, Term};

        let node_id = NodeId::new("follower-1");
        let storage = Arc::new(RaftStorage::new());
        let transport = Arc::new(InMemoryTransport::new());
        let config = RaftConfig::default();
        let engine = ConsensusEngine::new(node_id.clone(), config, storage, transport);

        // Populate the follower with 5 entries: E1-E3 at term 1, E4-E5 at term 2
        // (simulates entries from an old leader that were never committed).
        let mut initial_entries: Vec<LogEntry> = (1..=3)
            .map(|i| LogEntry {
                term: Term::new(1),
                index: LogIndex::new(i),
                data: vec![i as u8],
            })
            .collect();
        initial_entries.extend((4..=5).map(|i| LogEntry {
            term: Term::new(2),
            index: LogIndex::new(i),
            data: vec![i as u8],
        }));

        let populate = AppendEntriesRequest {
            term: Term::new(2),
            leader_id: NodeId::new("old-leader"),
            prev_log_index: LogIndex::new(0),
            prev_log_term: Term::new(0),
            entries: initial_entries,
            leader_commit: LogIndex::new(0),
        };
        let resp = engine.handle_append_entries(populate).await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.last_log_index, LogIndex::new(5));

        // New leader (term 3) sends two entries starting after E3.
        // E4 and E5 from term 2 conflict and must be replaced.
        let new_entries = vec![
            LogEntry {
                term: Term::new(3),
                index: LogIndex::new(4),
                data: vec![40],
            },
            LogEntry {
                term: Term::new(3),
                index: LogIndex::new(5),
                data: vec![50],
            },
        ];

        let replicate = AppendEntriesRequest {
            term: Term::new(3),
            leader_id: NodeId::new("new-leader"),
            prev_log_index: LogIndex::new(3),
            prev_log_term: Term::new(1),
            entries: new_entries,
            leader_commit: LogIndex::new(0),
        };
        let resp = engine.handle_append_entries(replicate).await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.last_log_index, LogIndex::new(5));

        // Critical scenario: another new leader sends only ONE entry after E3.
        // The follower currently has [E1(t1), E2(t1), E3(t1), E4(t3), E5(t3)].
        // After receiving [E4(t4)], the log must become [E1, E2, E3, E4_newer]
        // with E5(t3) truncated — it belongs to the previous leader's term.
        let shorter = AppendEntriesRequest {
            term: Term::new(4),
            leader_id: NodeId::new("newest-leader"),
            prev_log_index: LogIndex::new(3),
            prev_log_term: Term::new(1),
            entries: vec![LogEntry {
                term: Term::new(4),
                index: LogIndex::new(4),
                data: vec![44],
            }],
            leader_commit: LogIndex::new(0),
        };
        let resp = engine.handle_append_entries(shorter).await.unwrap();
        assert!(resp.success);
        // If truncation works, log is [E1, E2, E3, E4_newer] — length 4.
        // Before the fix this returned 5 because E5(t3) was left behind.
        assert_eq!(
            resp.last_log_index,
            LogIndex::new(4),
            "stale entry E5 from previous term must be truncated"
        );
    }
}
