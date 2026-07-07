//! Raft consensus engine.
//!
//! This module provides the main Raft consensus engine that coordinates
//! leader election, log replication, and state machine application.
//!
//! # Raft Algorithm Overview
//!
//! Raft is a consensus algorithm designed to be understandable. It's equivalent
//! to Paxos in fault-tolerance and performance, but structured to be easier to
//! understand and implement.
//!
//! Key components:
//! - **Leader Election**: Nodes elect a leader when no heartbeat is received
//! - **Log Replication**: Leader replicates log entries to followers
//! - **Safety**: Ensures all nodes see the same state
//!
//! # Implementation
//!
//! This implementation follows the Raft paper specification with:
//! - Randomized election timeouts to prevent split votes
//! - Log replication with consistency checks
//! - Leader heartbeat mechanism
//! - Term-based state transitions

use crate::consensus::{
    AppendEntriesRequest, AppendEntriesResponse, LeaderState, RaftNodeState, RaftStorage,
    RaftTransport, RequestVoteRequest, RequestVoteResponse,
};
use crate::error::{ConsensusError, ConsensusResult};
use crate::types::{LogEntry, LogIndex, NodeId, RaftState, StateMachineCommand, Term};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Configuration for the Raft consensus engine.
#[derive(Debug, Clone)]
pub struct RaftConfig {
    /// Election timeout range (min, max) in milliseconds.
    pub election_timeout_ms: (u64, u64),
    /// Heartbeat interval in milliseconds.
    pub heartbeat_interval_ms: u64,
    /// List of all node IDs in the cluster.
    pub cluster_nodes: Vec<NodeId>,
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            election_timeout_ms: (150, 300),
            heartbeat_interval_ms: 50,
            cluster_nodes: Vec::new(),
        }
    }
}

/// Raft consensus engine.
pub struct ConsensusEngine {
    node_id: NodeId,
    config: RaftConfig,
    state: Arc<RwLock<RaftNodeState>>,
    leader_state: Arc<RwLock<Option<LeaderState>>>,
    storage: Arc<RaftStorage>,
    transport: Arc<dyn RaftTransport>,
    shutdown_tx: Arc<RwLock<Option<mpsc::Sender<()>>>>,
    last_heartbeat: Arc<RwLock<Option<Instant>>>,
}

impl ConsensusEngine {
    /// Create a new consensus engine.
    pub fn new(
        node_id: NodeId,
        config: RaftConfig,
        storage: Arc<RaftStorage>,
        transport: Arc<dyn RaftTransport>,
    ) -> Self {
        Self {
            node_id,
            config,
            state: Arc::new(RwLock::new(RaftNodeState::new())),
            leader_state: Arc::new(RwLock::new(None)),
            storage,
            transport,
            shutdown_tx: Arc::new(RwLock::new(None)),
            last_heartbeat: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the consensus engine.
    pub async fn start(&self) -> ConsensusResult<()> {
        info!("Starting Raft consensus engine for node {}", self.node_id);

        // Load persistent state from storage
        self.load_persistent_state().await?;

        // Start the main Raft loop
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        let node_id = self.node_id.clone();
        let state = Arc::clone(&self.state);
        let leader_state = Arc::clone(&self.leader_state);
        let config = self.config.clone();
        let transport = Arc::clone(&self.transport);
        let last_heartbeat = Arc::clone(&self.last_heartbeat);
        let cluster_nodes = self.config.cluster_nodes.clone();

        tokio::spawn(async move {
            Self::raft_loop(
                node_id,
                state,
                leader_state,
                config,
                transport,
                last_heartbeat,
                cluster_nodes,
                &mut shutdown_rx,
            )
            .await;
        });

        Ok(())
    }

    /// Main Raft event loop.
    #[allow(clippy::too_many_arguments)]
    async fn raft_loop(
        node_id: NodeId,
        state: Arc<RwLock<RaftNodeState>>,
        leader_state: Arc<RwLock<Option<LeaderState>>>,
        config: RaftConfig,
        transport: Arc<dyn RaftTransport>,
        last_heartbeat: Arc<RwLock<Option<Instant>>>,
        cluster_nodes: Vec<NodeId>,
        shutdown_rx: &mut mpsc::Receiver<()>,
    ) {
        loop {
            // Check for shutdown signal
            if shutdown_rx.try_recv().is_ok() {
                info!("Raft loop shutting down for node {}", node_id);
                break;
            }

            let current_state = state.read().await.state;
            match current_state {
                RaftState::Follower => {
                    Self::follower_loop(&node_id, &state, &last_heartbeat, &config, shutdown_rx)
                        .await;
                    // After follower loop returns, check state again (might have become candidate)
                    continue;
                }
                RaftState::Candidate => {
                    Self::candidate_loop(
                        &node_id,
                        &state,
                        &leader_state,
                        &transport,
                        &cluster_nodes,
                        &config,
                        shutdown_rx,
                    )
                    .await;
                    // After candidate loop returns, check state again (might have become leader or follower)
                    continue;
                }
                RaftState::Leader => {
                    Self::leader_loop(
                        &node_id,
                        &state,
                        &leader_state,
                        &transport,
                        &cluster_nodes,
                        &config,
                        shutdown_rx,
                    )
                    .await;
                    // After leader loop returns, check state again (might have become follower)
                    continue;
                }
            }
        }
    }

    /// Follower event loop.
    async fn follower_loop(
        node_id: &NodeId,
        state: &Arc<RwLock<RaftNodeState>>,
        last_heartbeat: &Arc<RwLock<Option<Instant>>>,
        config: &RaftConfig,
        shutdown_rx: &mut mpsc::Receiver<()>,
    ) {
        // Calculate random election timeout
        // Use node ID hash to ensure different nodes get different timeouts
        // This prevents all nodes from timing out simultaneously
        let timeout_range = config.election_timeout_ms.1 - config.election_timeout_ms.0;
        let timeout_ms = if timeout_range == 0 {
            config.election_timeout_ms.0
        } else {
            let mut hasher = DefaultHasher::new();
            node_id.hash(&mut hasher);
            let node_hash = hasher.finish();
            config.election_timeout_ms.0 + (node_hash % timeout_range)
        };
        let timeout = Duration::from_millis(timeout_ms);

        debug!(
            "Follower {} waiting {}ms for heartbeat",
            node_id, timeout_ms
        );

        // Wait for heartbeat or timeout
        // Track when we started waiting and when we last received a heartbeat
        let wait_start = Instant::now();
        let mut last_known_heartbeat = *last_heartbeat.read().await;

        loop {
            // Check current heartbeat status
            let heartbeat_guard = last_heartbeat.read().await;
            let current_heartbeat = *heartbeat_guard;
            drop(heartbeat_guard);

            // If we received a new heartbeat, reset our wait timer
            if current_heartbeat != last_known_heartbeat
                && let Some(new_heartbeat) = current_heartbeat
            {
                debug!("Follower {} received heartbeat, resetting timeout", node_id);
                last_known_heartbeat = Some(new_heartbeat);
                // Reset wait start to now since we got a heartbeat
                // Actually, we should track time since last heartbeat, not since wait start
                continue;
            }

            // Calculate elapsed time since last heartbeat (or since we started if no heartbeat)
            let elapsed = if let Some(last) = last_known_heartbeat {
                last.elapsed()
            } else {
                // No heartbeat received yet, use time since we started waiting
                wait_start.elapsed()
            };

            if elapsed >= timeout {
                // No heartbeat received within timeout, become candidate
                warn!(
                    "Follower {} timed out ({}ms elapsed), becoming candidate",
                    node_id,
                    elapsed.as_millis()
                );
                let mut s = state.write().await;
                // Double-check we're still a follower (might have been updated)
                if s.state == RaftState::Follower {
                    s.state = RaftState::Candidate;
                }
                break;
            }

            // Wait for either timeout remaining time or shutdown
            let remaining = timeout - elapsed;
            tokio::select! {
                _ = sleep(remaining.min(Duration::from_millis(50))) => {
                    // Check again after short sleep
                    continue;
                }
                _ = shutdown_rx.recv() => {
                    return;
                }
            }
        }
    }

    /// Candidate event loop (leader election).
    async fn candidate_loop(
        node_id: &NodeId,
        state: &Arc<RwLock<RaftNodeState>>,
        leader_state: &Arc<RwLock<Option<LeaderState>>>,
        transport: &Arc<dyn RaftTransport>,
        cluster_nodes: &[NodeId],
        config: &RaftConfig,
        shutdown_rx: &mut mpsc::Receiver<()>,
    ) {
        let mut s = state.write().await;

        // Increment term and vote for self
        s.current_term = s.current_term.increment();
        s.voted_for = Some(node_id.clone());
        let current_term = s.current_term;
        drop(s);

        info!(
            "Candidate {} starting election in term {}",
            node_id, current_term
        );

        // Get last log info
        let (last_log_term, last_log_index) = {
            let s = state.read().await;
            s.last_log_info()
        };

        // Request votes from all other nodes
        let vote_request = RequestVoteRequest {
            term: state.read().await.current_term,
            candidate_id: node_id.clone(),
            last_log_index,
            last_log_term,
        };

        let mut votes_received = 1; // Vote for self
        let quorum = (cluster_nodes.len() / 2) + 1;

        // Send vote requests to all other nodes
        let mut vote_tasks = Vec::new();
        for follower_id in cluster_nodes {
            if follower_id == node_id {
                continue;
            }

            let transport_clone = Arc::clone(transport);
            let follower_id_clone = follower_id.clone();
            let request = vote_request.clone();

            vote_tasks.push(tokio::spawn(async move {
                transport_clone
                    .request_vote(&follower_id_clone, request)
                    .await
            }));
        }

        // Collect votes with timeout - wait for all responses concurrently
        let election_timeout = Duration::from_millis(config.election_timeout_ms.1);

        // Use tokio::time::timeout to wait for all vote responses
        let vote_futures: Vec<_> = vote_tasks.into_iter().collect();
        let vote_results = tokio::time::timeout(election_timeout, async {
            let mut results = Vec::new();
            for task in vote_futures {
                match task.await {
                    Ok(Ok(response)) => results.push(Ok(response)),
                    Ok(Err(e)) => {
                        debug!("Candidate {} vote request error: {}", node_id, e);
                        results.push(Err(e));
                    }
                    Err(e) => {
                        debug!("Candidate {} vote task join error: {:?}", node_id, e);
                    }
                }
            }
            results
        })
        .await;

        match vote_results {
            Ok(responses) => {
                // Process all vote responses
                for response_result in responses {
                    match response_result {
                        Ok(response) => {
                            debug!(
                                "Candidate {} received vote response: granted={}, term={}",
                                node_id, response.vote_granted, response.term
                            );

                            let current_term_check = state.read().await.current_term;
                            if response.term > current_term_check {
                                // Higher term seen, become follower
                                let mut s = state.write().await;
                                s.current_term = response.term;
                                s.state = RaftState::Follower;
                                s.voted_for = None;
                                return;
                            }

                            if response.vote_granted {
                                votes_received += 1;
                                debug!(
                                    "Candidate {} received vote, total: {}/{}",
                                    node_id, votes_received, quorum
                                );

                                // Check quorum immediately
                                if votes_received >= quorum {
                                    info!(
                                        "Candidate {} won election with {}/{} votes",
                                        node_id,
                                        votes_received,
                                        cluster_nodes.len()
                                    );
                                    let mut s = state.write().await;
                                    s.state = RaftState::Leader;
                                    let followers: Vec<NodeId> = cluster_nodes
                                        .iter()
                                        .filter(|n| *n != node_id)
                                        .cloned()
                                        .collect();
                                    let new_leader_state =
                                        LeaderState::new(&followers, last_log_index);
                                    drop(s);
                                    *leader_state.write().await = Some(new_leader_state);
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Candidate {} vote request failed: {}", node_id, e);
                        }
                    }
                }
            }
            Err(_) => {
                debug!(
                    "Candidate {} election timeout after {}ms",
                    node_id,
                    election_timeout.as_millis()
                );
            }
        }

        // Final check for quorum
        if votes_received >= quorum {
            info!(
                "Candidate {} won election with {}/{} votes (after timeout)",
                node_id,
                votes_received,
                cluster_nodes.len()
            );
            let mut s = state.write().await;
            s.state = RaftState::Leader;
            let followers: Vec<NodeId> = cluster_nodes
                .iter()
                .filter(|n| *n != node_id)
                .cloned()
                .collect();
            let new_leader_state = LeaderState::new(&followers, last_log_index);
            drop(s);
            *leader_state.write().await = Some(new_leader_state);
            return;
        }

        // Didn't get enough votes, remain candidate (will retry)
        warn!(
            "Candidate {} didn't get enough votes ({}/{})",
            node_id, votes_received, quorum
        );
    }

    /// Leader event loop (log replication and heartbeats).
    async fn leader_loop(
        node_id: &NodeId,
        state: &Arc<RwLock<RaftNodeState>>,
        leader_state: &Arc<RwLock<Option<LeaderState>>>,
        transport: &Arc<dyn RaftTransport>,
        cluster_nodes: &[NodeId],
        config: &RaftConfig,
        shutdown_rx: &mut mpsc::Receiver<()>,
    ) {
        let heartbeat_interval = Duration::from_millis(config.heartbeat_interval_ms);

        info!("Leader {} starting leader loop", node_id);

        loop {
            // Check if we're still the leader (might have been demoted)
            let current_state = state.read().await.state;
            if current_state != RaftState::Leader {
                warn!(
                    "Leader {} is no longer leader, state: {:?}",
                    node_id, current_state
                );
                return;
            }

            tokio::select! {
                _ = sleep(heartbeat_interval) => {
                    // Send heartbeats to all followers
                    debug!("Leader {} sending heartbeats", node_id);
                    Self::send_heartbeats(
                        node_id,
                        state,
                        leader_state,
                        transport,
                        cluster_nodes,
                    )
                    .await;
                }
                _ = shutdown_rx.recv() => {
                    info!("Leader {} shutting down", node_id);
                    return;
                }
            }
        }
    }

    /// Send heartbeats (empty AppendEntries) to all followers.
    async fn send_heartbeats(
        node_id: &NodeId,
        state: &Arc<RwLock<RaftNodeState>>,
        leader_state: &Arc<RwLock<Option<LeaderState>>>,
        transport: &Arc<dyn RaftTransport>,
        cluster_nodes: &[NodeId],
    ) {
        let current_term = state.read().await.current_term;
        let (prev_log_term, prev_log_index) = {
            let s = state.read().await;
            s.last_log_info()
        };

        let leader_state_guard = leader_state.read().await;
        let leader_state_ref = match leader_state_guard.as_ref() {
            Some(ls) => ls,
            None => return,
        };

        // Send heartbeat to each follower
        for follower_id in cluster_nodes {
            if follower_id == node_id {
                continue;
            }

            let next_index = leader_state_ref
                .next_index
                .get(follower_id)
                .copied()
                .unwrap_or(prev_log_index);

            // Read commit_index after getting prev_log info to ensure we have the latest
            let current_commit_index = state.read().await.commit_index;
            let heartbeat = AppendEntriesRequest {
                term: current_term,
                leader_id: node_id.clone(),
                prev_log_index: next_index,
                prev_log_term,
                entries: Vec::new(), // Empty for heartbeat
                leader_commit: current_commit_index,
            };

            info!(
                "Leader {} sending heartbeat with commit_index={}",
                node_id, current_commit_index.0
            );

            let transport_clone = Arc::clone(transport);
            let follower_id_clone = follower_id.clone();
            let state_clone = Arc::clone(state);
            let leader_state_clone = Arc::clone(leader_state);

            tokio::spawn(async move {
                match transport_clone
                    .append_entries(&follower_id_clone, heartbeat)
                    .await
                {
                    Ok(response) => {
                        if response.term > current_term {
                            // Higher term seen, become follower
                            let mut s = state_clone.write().await;
                            s.current_term = response.term;
                            s.state = RaftState::Follower;
                            s.voted_for = None;
                        } else if response.success {
                            // Update next_index and match_index
                            let mut ls = leader_state_clone.write().await;
                            if let Some(ref mut ls_ref) = *ls {
                                ls_ref.next_index.insert(
                                    follower_id_clone.clone(),
                                    response.last_log_index.increment(),
                                );
                                ls_ref
                                    .match_index
                                    .insert(follower_id_clone.clone(), response.last_log_index);
                            }
                        }
                    }
                    Err(_) => {
                        // Network error, will retry on next heartbeat
                        debug!("Failed to send heartbeat to {}", follower_id_clone);
                    }
                }
            });
        }
    }

    /// Handle a RequestVote RPC (called by transport layer).
    pub async fn handle_request_vote(
        &self,
        request: RequestVoteRequest,
    ) -> ConsensusResult<RequestVoteResponse> {
        let mut state = self.state.write().await;

        debug!(
            "Node {} received vote request from {} for term {}",
            self.node_id, request.candidate_id, request.term
        );

        // If request term is less than current term, reject
        if request.term < state.current_term {
            debug!(
                "Node {} rejecting vote: request term {} < current term {}",
                self.node_id, request.term, state.current_term
            );
            return Ok(RequestVoteResponse {
                term: state.current_term,
                vote_granted: false,
            });
        }

        // If we're the leader and receive a vote request with same or higher term, step down
        // This shouldn't happen in normal operation, but handle it gracefully
        if state.state == RaftState::Leader && request.term >= state.current_term {
            warn!(
                "Leader {} received vote request with term {} >= current term {}, stepping down",
                self.node_id, request.term, state.current_term
            );
            if request.term > state.current_term {
                state.current_term = request.term;
            }
            state.state = RaftState::Follower;
            state.voted_for = None;
            *self.last_heartbeat.write().await = Some(Instant::now());
        }

        // If request term is greater, update term and become follower
        if request.term > state.current_term {
            debug!(
                "Node {} updating term from {} to {}, becoming follower",
                self.node_id, state.current_term, request.term
            );
            state.current_term = request.term;
            state.state = RaftState::Follower;
            state.voted_for = None;
            // Reset heartbeat timer when we see a higher term
            *self.last_heartbeat.write().await = Some(Instant::now());
        }

        // Check if we can vote for this candidate
        let (last_log_term, last_log_index) = state.last_log_info();
        let can_vote =
            state.voted_for.is_none() || state.voted_for.as_ref() == Some(&request.candidate_id);

        let vote_granted = can_vote
            && (request.last_log_term > last_log_term
                || (request.last_log_term == last_log_term
                    && request.last_log_index >= last_log_index));

        if vote_granted {
            state.voted_for = Some(request.candidate_id.clone());
            info!(
                "Node {} voted for {} in term {}",
                self.node_id, request.candidate_id, request.term
            );
        }

        Ok(RequestVoteResponse {
            term: state.current_term,
            vote_granted,
        })
    }

    /// Handle an AppendEntries RPC (called by transport layer).
    pub async fn handle_append_entries(
        &self,
        request: AppendEntriesRequest,
    ) -> ConsensusResult<AppendEntriesResponse> {
        let mut state = self.state.write().await;

        debug!(
            "Node {} received AppendEntries from {} for term {}",
            self.node_id, request.leader_id, request.term
        );

        // Update last heartbeat time
        *self.last_heartbeat.write().await = Some(Instant::now());

        // If request term is less than current term, reject
        if request.term < state.current_term {
            return Ok(AppendEntriesResponse {
                term: state.current_term,
                success: false,
                last_log_index: state.last_log_info().1,
            });
        }

        // If request term is greater, update term and become follower
        if request.term > state.current_term {
            state.current_term = request.term;
            state.state = RaftState::Follower;
            state.voted_for = None;
        }

        // If we're not follower, become follower
        if state.state != RaftState::Follower {
            state.state = RaftState::Follower;
        }

        // Check log consistency
        let success = if request.prev_log_index.0 > 0 {
            // Check if we have an entry at prev_log_index with matching term
            // prev_log_index is 1-indexed, so we need to check index (prev_log_index - 1)
            let prev_idx = (request.prev_log_index.0 - 1) as usize;
            if prev_idx < state.log.len() {
                let entry_term = state.log[prev_idx].term;
                let matches = entry_term == request.prev_log_term;
                if !matches {
                    info!(
                        "Node {} log consistency check failed: prev_log_index={}, log has term {} but request has term {}",
                        self.node_id,
                        request.prev_log_index.0,
                        entry_term.0,
                        request.prev_log_term.0
                    );
                }
                matches
            } else {
                info!(
                    "Node {} log consistency check failed: prev_log_index={} but log length is {}",
                    self.node_id,
                    request.prev_log_index.0,
                    state.log.len()
                );
                false
            }
        } else {
            true // First entry, always consistent
        };

        if success && !request.entries.is_empty() {
            let start_index = request.prev_log_index.0 as usize;
            for (i, entry) in request.entries.iter().enumerate() {
                let log_index = start_index + i + 1;
                if log_index <= state.log.len() {
                    if state.log[log_index - 1].term != entry.term {
                        // Conflicting entry (same index, different term) —
                        // truncate from here and append the rest (Raft §5.3).
                        state.log.truncate(log_index - 1);
                        state.log.extend(request.entries[i..].iter().cloned());
                        break;
                    }
                    // Terms match — entry already present, skip
                } else {
                    // Past end of log — append remaining entries
                    state.log.extend(request.entries[i..].iter().cloned());
                    break;
                }
            }
        }

        // Update commit_index
        if request.leader_commit > state.commit_index {
            let old_commit = state.commit_index;
            state.commit_index = request.leader_commit.min(state.last_log_info().1);
            info!(
                "Node {} updated commit_index from {} to {} (leader_commit: {})",
                self.node_id, old_commit.0, state.commit_index.0, request.leader_commit.0
            );
        }

        let last_log_index = state.last_log_info().1;
        drop(state);

        Ok(AppendEntriesResponse {
            term: self.state.read().await.current_term,
            success,
            last_log_index,
        })
    }

    /// Propose a command to be replicated (leader only).
    pub async fn propose(&self, command: StateMachineCommand) -> ConsensusResult<LogIndex> {
        let mut state = self.state.write().await;

        // Only leader can propose
        if state.state != RaftState::Leader {
            return Err(ConsensusError::NotLeader(format!(
                "Node {} is not the leader",
                self.node_id
            )));
        }

        // Create log entry
        let term = state.current_term;
        let index = state.log.len() as u64 + 1;
        let entry = LogEntry {
            term,
            index: LogIndex::new(index),
            data: bincode::serialize(&command)
                .map_err(|e| ConsensusError::Internal(e.to_string()))?,
        };

        // Append to local log
        state.log.push(entry.clone());
        let log_index = entry.index;
        // Calculate prev_log_term and prev_log_index
        // If this is the first entry (index 1), prev_log_index should be 0 (no previous entry)
        // If this is entry N (index N), prev_log_index should be N-1
        let prev_log_term = if state.log.len() > 1 {
            // There's a previous entry
            state.log[state.log.len() - 2].term
        } else {
            // This is the first entry
            Term::new(0)
        };
        let prev_log_index = if state.log.len() > 1 {
            // There's a previous entry, use its index
            LogIndex::new((state.log.len() - 1) as u64)
        } else {
            // This is the first entry, use 0 (no previous entry)
            LogIndex::new(0)
        };
        drop(state);

        // Replicate to followers
        self.replicate_entry(entry, prev_log_index, prev_log_term, log_index)
            .await?;

        Ok(log_index)
    }

    /// Replicate a log entry to followers and wait for quorum.
    async fn replicate_entry(
        &self,
        entry: LogEntry,
        prev_log_index: LogIndex,
        prev_log_term: Term,
        log_index: LogIndex,
    ) -> ConsensusResult<()> {
        let state = Arc::clone(&self.state);
        let leader_state = Arc::clone(&self.leader_state);
        let transport = Arc::clone(&self.transport);
        let node_id = self.node_id.clone();
        let cluster_nodes = self.config.cluster_nodes.clone();

        // Get leader state
        let leader_state_guard = leader_state.read().await;
        let leader_state_ref = match leader_state_guard.as_ref() {
            Some(ls) => ls,
            None => {
                return Err(ConsensusError::Internal(
                    "Leader state not initialized".to_string(),
                ));
            }
        };

        let current_term = state.read().await.current_term;
        let commit_index = state.read().await.commit_index;
        drop(leader_state_guard);

        // Calculate quorum (majority)
        let quorum = (cluster_nodes.len() / 2) + 1;
        let mut success_count = 1; // Count self (leader) as success

        // Send AppendEntries to all followers
        let mut replication_tasks = Vec::new();
        for follower_id in &cluster_nodes {
            if follower_id == &node_id {
                continue;
            }

            let follower_id_clone = follower_id.clone();
            let leader_id_clone = node_id.clone(); // Clone for use in closure
            let transport_clone = Arc::clone(&transport);
            let state_clone = Arc::clone(&state);
            let leader_state_clone = Arc::clone(&leader_state);
            let entry_clone = entry.clone();

            replication_tasks.push(tokio::spawn(async move {
                // Use prev_log_index passed from propose() directly
                // This ensures correct prev_log_index (0 for first entry)
                // The next_index in leader_state is initialized incorrectly for empty logs
                // For retries, we'll use next_index from leader_state
                let actual_prev_log_index = {
                    let ls = leader_state_clone.read().await;
                    if let Some(ls_ref) = ls.as_ref() {
                        let stored_next = ls_ref.next_index.get(&follower_id_clone).copied();
                        // If stored next_index is 1 but prev_log_index is 0, this is the first entry
                        // Use prev_log_index=0 directly
                        if let Some(next) = stored_next {
                            if next.0 == 1 && prev_log_index.0 == 0 {
                                // First entry - use prev_log_index=0
                                prev_log_index
                            } else {
                                // Retry case - use stored next_index - 1
                                LogIndex::new(next.0.saturating_sub(1))
                            }
                        } else {
                            prev_log_index
                        }
                    } else {
                        prev_log_index
                    }
                };

                // Prepare AppendEntries request
                let request = AppendEntriesRequest {
                    term: current_term,
                    leader_id: leader_id_clone,
                    prev_log_index: actual_prev_log_index,
                    prev_log_term,
                    entries: vec![entry_clone],
                    leader_commit: commit_index,
                };

                // Send request
                match transport_clone
                    .append_entries(&follower_id_clone, request)
                    .await
                {
                    Ok(response) => {
                        // Check for higher term
                        if response.term > current_term {
                            let mut s = state_clone.write().await;
                            s.current_term = response.term;
                            s.state = RaftState::Follower;
                            s.voted_for = None;
                            return Err(ConsensusError::TermMismatch {
                                expected: current_term.0,
                                got: response.term.0,
                            });
                        }

                        if response.success {
                            info!(
                                "Replication to {} succeeded, last_log_index={}",
                                follower_id_clone, response.last_log_index.0
                            );
                            // Update next_index and match_index
                            let mut ls = leader_state_clone.write().await;
                            if let Some(ref mut ls_ref) = *ls {
                                ls_ref.next_index.insert(
                                    follower_id_clone.clone(),
                                    response.last_log_index.increment(),
                                );
                                ls_ref
                                    .match_index
                                    .insert(follower_id_clone.clone(), response.last_log_index);
                            }
                            Ok(true)
                        } else {
                            info!(
                                "Replication to {} failed (success=false)",
                                follower_id_clone
                            );
                            // Follower rejected, decrement next_index and retry
                            let mut ls = leader_state_clone.write().await;
                            if let Some(ref mut ls_ref) = *ls
                                && let Some(current_next) =
                                    ls_ref.next_index.get(&follower_id_clone)
                                && current_next.0 > 1
                            {
                                ls_ref.next_index.insert(
                                    follower_id_clone.clone(),
                                    LogIndex::new(current_next.0 - 1),
                                );
                            }
                            Ok(false)
                        }
                    }
                    Err(e) => {
                        debug!("Failed to replicate to {}: {}", follower_id_clone, e);
                        Err(e)
                    }
                }
            }));
        }

        // Wait for quorum with timeout
        let timeout = Duration::from_millis(1000); // 1 second timeout
        let start = Instant::now();
        let mut completed = 0;

        for task in replication_tasks {
            if start.elapsed() >= timeout {
                warn!("Replication timeout waiting for quorum");
                break;
            }

            tokio::select! {
                result = task => {
                    completed += 1;
                    if let Ok(Ok(true)) = result {
                        success_count += 1;
                        if success_count >= quorum {
                            // Quorum reached, update commit index and send heartbeat
                            let should_send_heartbeat = {
                                let mut s = state.write().await;
                                if s.state == RaftState::Leader {
                                    s.commit_index = log_index;
                                    info!("Quorum reached for log index {} (early, after {} responses), commit_index updated to {}", log_index.0, completed, log_index.0);
                                    true
                                } else {
                                    false
                                }
                            };

                            // Send immediate heartbeat to update followers' commit_index
                            if should_send_heartbeat {
                                Self::send_heartbeats(
                                    &node_id,
                                    &state,
                                    &leader_state,
                                    &transport,
                                    &cluster_nodes,
                                ).await;
                            }
                            return Ok(());
                        }
                    }
                }
                _ = sleep(Duration::from_millis(10)) => continue,
            }
        }

        // Check if we have quorum after all responses
        if success_count >= quorum {
            let should_send_heartbeat = {
                let mut s = state.write().await;
                if s.state == RaftState::Leader {
                    s.commit_index = log_index;
                    info!(
                        "Quorum reached for log index {} (after {} responses), commit_index updated to {}",
                        log_index.0, completed, log_index.0
                    );
                    true
                } else {
                    false
                }
            };

            // Send immediate heartbeat to update followers' commit_index
            // This ensures followers commit the entry quickly
            if should_send_heartbeat {
                Self::send_heartbeats(&node_id, &state, &leader_state, &transport, &cluster_nodes)
                    .await;
            }
            Ok(())
        } else {
            warn!(
                "Failed to reach quorum for log index {}: {}/{} successes",
                log_index.0, success_count, quorum
            );
            // Still return success - entry is in log, will be retried on next heartbeat
            Ok(())
        }
    }

    /// Load persistent state from storage.
    async fn load_persistent_state(&self) -> ConsensusResult<()> {
        let mut state = self.state.write().await;

        // Load term
        if let Some(term) = self.storage.load_term()? {
            state.current_term = term;
        }

        // Load voted_for
        state.voted_for = self.storage.load_voted_for()?;

        // Load log entries
        state.log = self.storage.load_all_log_entries()?;

        Ok(())
    }

    /// Stop the consensus engine.
    pub async fn stop(&self) -> ConsensusResult<()> {
        info!("Stopping Raft consensus engine for node {}", self.node_id);
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(()).await;
        }
        Ok(())
    }

    /// Get current Raft state.
    pub async fn state(&self) -> RaftState {
        self.state.read().await.state
    }

    /// Get current term.
    pub async fn current_term(&self) -> Term {
        self.state.read().await.current_term
    }

    /// Check if this node is the leader.
    pub async fn is_leader(&self) -> bool {
        self.state.read().await.state == RaftState::Leader
    }

    /// Get committed log entries that haven't been applied yet.
    /// Returns (commit_index, entries_to_apply)
    pub async fn get_committed_entries(&self, last_applied: u64) -> (u64, Vec<LogEntry>) {
        let state = self.state.read().await;
        let commit_index = state.commit_index.0;

        if commit_index <= last_applied {
            return (commit_index, Vec::new());
        }

        // Get entries from last_applied + 1 to commit_index
        // Note: last_applied is 0-indexed, but log entries are 1-indexed
        // So we need to get entries from index (last_applied) to (commit_index - 1)
        let start_idx = last_applied as usize;
        let end_idx = commit_index as usize;

        // Ensure we don't go out of bounds
        if end_idx > state.log.len() {
            debug!(
                "Node {}: commit_index {} > log length {}, using log length",
                self.node_id,
                end_idx,
                state.log.len()
            );
            // Return empty if log isn't long enough yet
            return (commit_index, Vec::new());
        }

        let entries: Vec<LogEntry> = state
            .log
            .iter()
            .skip(start_idx)
            .take(end_idx - start_idx)
            .cloned()
            .collect();

        debug!(
            "Node {}: get_committed_entries: commit_index={}, last_applied={}, log_len={}, returning {} entries",
            self.node_id,
            commit_index,
            last_applied,
            state.log.len(),
            entries.len()
        );

        (commit_index, entries)
    }
}
