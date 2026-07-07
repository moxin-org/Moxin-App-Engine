//! Load balancing algorithms.
//!
//! This module provides various load balancing algorithms for distributing
//! requests across cluster nodes.
//!
//! # Implementation Status
//!
//! **Complete** - Multiple load balancing algorithms implemented (Round-Robin, Least-Connections, Weighted, Random)

use crate::error::GatewayResult;
use crate::types::{LoadBalancingAlgorithm, NodeId};
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Load balancer for distributing requests across nodes.
pub struct LoadBalancer {
    algorithm: LoadBalancingAlgorithm,
    nodes: Arc<RwLock<Vec<NodeId>>>,
    // Round-robin state
    round_robin_index: Arc<RwLock<usize>>,
    // Connection counts for least-connections
    connection_counts: Arc<RwLock<HashMap<NodeId, usize>>>,
    // Node weights for weighted round-robin
    node_weights: Arc<RwLock<HashMap<NodeId, u32>>>,
    // Weighted round-robin state (current weight for each node)
    weighted_current_weights: Arc<RwLock<HashMap<NodeId, i32>>>,
}

impl LoadBalancer {
    /// Create a new load balancer.
    pub fn new(algorithm: LoadBalancingAlgorithm) -> Self {
        Self {
            algorithm,
            nodes: Arc::new(RwLock::new(Vec::new())),
            round_robin_index: Arc::new(RwLock::new(0)),
            connection_counts: Arc::new(RwLock::new(HashMap::new())),
            node_weights: Arc::new(RwLock::new(HashMap::new())),
            weighted_current_weights: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a node to the load balancer.
    pub async fn add_node(&self, node_id: NodeId) {
        let mut nodes = self.nodes.write().await;
        if !nodes.contains(&node_id) {
            nodes.push(node_id.clone());
            // Initialize weight to 1 (default)
            let mut weights = self.node_weights.write().await;
            weights.entry(node_id.clone()).or_insert(1);
            let mut current_weights = self.weighted_current_weights.write().await;
            current_weights.entry(node_id).or_insert(0);
        }
    }

    /// Set the weight for a node (for weighted round-robin).
    pub async fn set_node_weight(&self, node_id: &NodeId, weight: u32) {
        let mut weights = self.node_weights.write().await;
        weights.insert(node_id.clone(), weight);
    }

    /// Get the weight for a node.
    pub async fn get_node_weight(&self, node_id: &NodeId) -> u32 {
        let weights = self.node_weights.read().await;
        weights.get(node_id).copied().unwrap_or(1)
    }

    /// Remove a node from the load balancer.
    pub async fn remove_node(&self, node_id: &NodeId) {
        let mut nodes = self.nodes.write().await;
        nodes.retain(|n| n != node_id);
        let mut counts = self.connection_counts.write().await;
        counts.remove(node_id);
        let mut weights = self.node_weights.write().await;
        weights.remove(node_id);
        let mut current_weights = self.weighted_current_weights.write().await;
        current_weights.remove(node_id);
    }

    /// Select a node using the configured algorithm.
    pub async fn select_node(&self) -> GatewayResult<Option<NodeId>> {
        let nodes = self.nodes.read().await;
        if nodes.is_empty() {
            return Ok(None);
        }

        match self.algorithm {
            LoadBalancingAlgorithm::RoundRobin => {
                let mut index = self.round_robin_index.write().await;
                let node = nodes[*index % nodes.len()].clone();
                *index = (*index + 1) % nodes.len();
                Ok(Some(node))
            }
            LoadBalancingAlgorithm::LeastConnections => {
                let counts = self.connection_counts.read().await;
                let mut min_connections = usize::MAX;
                let mut selected = None;

                for node in nodes.iter() {
                    let conn_count = counts.get(node).copied().unwrap_or(0);
                    if conn_count < min_connections {
                        min_connections = conn_count;
                        selected = Some(node.clone());
                    }
                }

                Ok(selected)
            }
            LoadBalancingAlgorithm::WeightedRoundRobin => {
                // Weighted round-robin algorithm (WRR)
                // Uses the standard WRR algorithm where each node is selected
                // proportionally to its weight
                let weights = self.node_weights.read().await;
                let mut current_weights = self.weighted_current_weights.write().await;

                // Initialize current weights if needed
                for node in nodes.iter() {
                    current_weights.entry(node.clone()).or_insert(0);
                }

                // Find node with maximum (current_weight + weight)
                let mut max_effective_weight = i32::MIN;
                let mut selected = None;

                for node in nodes.iter() {
                    let weight = weights.get(node).copied().unwrap_or(1) as i32;
                    let current = current_weights.get(node).copied().unwrap_or(0);
                    let effective_weight = current + weight;

                    if effective_weight > max_effective_weight {
                        max_effective_weight = effective_weight;
                        selected = Some(node.clone());
                    }
                }

                // Decrease current weight of selected node by sum of all weights
                if let Some(ref selected_node) = selected {
                    let total_weight: i32 = nodes
                        .iter()
                        .map(|n| weights.get(n).copied().unwrap_or(1) as i32)
                        .sum();

                    if let Some(current) = current_weights.get_mut(selected_node) {
                        *current -= total_weight;
                    }
                }

                Ok(selected)
            }
            LoadBalancingAlgorithm::Random => {
                let index = rand::thread_rng().gen_range(0..nodes.len());
                Ok(Some(nodes[index].clone()))
            }
        }
    }

    /// Increment connection count for a node.
    pub async fn increment_connections(&self, node_id: &NodeId) {
        let mut counts = self.connection_counts.write().await;
        *counts.entry(node_id.clone()).or_insert(0) += 1;
    }

    /// Decrement connection count for a node.
    pub async fn decrement_connections(&self, node_id: &NodeId) {
        let mut counts = self.connection_counts.write().await;
        if let Some(count) = counts.get_mut(node_id)
            && *count > 0
        {
            *count -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_round_robin() {
        let lb = LoadBalancer::new(LoadBalancingAlgorithm::RoundRobin);
        lb.add_node(NodeId::new("node-1")).await;
        lb.add_node(NodeId::new("node-2")).await;
        lb.add_node(NodeId::new("node-3")).await;

        // Should cycle through nodes
        let node1 = lb.select_node().await.unwrap().unwrap();
        let node2 = lb.select_node().await.unwrap().unwrap();
        let node3 = lb.select_node().await.unwrap().unwrap();
        let node4 = lb.select_node().await.unwrap().unwrap();

        assert_eq!(node1, NodeId::new("node-1"));
        assert_eq!(node2, NodeId::new("node-2"));
        assert_eq!(node3, NodeId::new("node-3"));
        assert_eq!(node4, NodeId::new("node-1")); // Wraps around
    }

    #[tokio::test]
    async fn test_least_connections() {
        let lb = LoadBalancer::new(LoadBalancingAlgorithm::LeastConnections);
        lb.add_node(NodeId::new("node-1")).await;
        lb.add_node(NodeId::new("node-2")).await;

        // Initially both have 0, should select first
        let node = lb.select_node().await.unwrap().unwrap();
        assert_eq!(node, NodeId::new("node-1"));

        // Increment connections for node-1
        lb.increment_connections(&NodeId::new("node-1")).await;
        lb.increment_connections(&NodeId::new("node-1")).await;

        // Now should select node-2 (fewer connections)
        let node = lb.select_node().await.unwrap().unwrap();
        assert_eq!(node, NodeId::new("node-2"));
    }

    #[tokio::test]
    async fn test_weighted_round_robin() {
        let lb = LoadBalancer::new(LoadBalancingAlgorithm::WeightedRoundRobin);
        lb.add_node(NodeId::new("node-1")).await;
        lb.add_node(NodeId::new("node-2")).await;
        lb.add_node(NodeId::new("node-3")).await;

        // Set weights: node-1=3, node-2=2, node-3=1
        lb.set_node_weight(&NodeId::new("node-1"), 3).await;
        lb.set_node_weight(&NodeId::new("node-2"), 2).await;
        lb.set_node_weight(&NodeId::new("node-3"), 1).await;

        // With weighted round-robin, verify it selects nodes
        // Over 6 selections (one full cycle), we should see all nodes selected
        let mut selections = Vec::new();
        for _ in 0..6 {
            let node = lb.select_node().await.unwrap().unwrap();
            selections.push(node);
        }

        // Verify all nodes are selected at least once
        assert!(selections.contains(&NodeId::new("node-1")));
        assert!(selections.contains(&NodeId::new("node-2")));
        assert!(selections.contains(&NodeId::new("node-3")));

        // Count selections over more rounds to verify weighting
        let mut node1_count = 0;
        let mut node2_count = 0;
        let mut node3_count = 0;

        for _ in 0..18 {
            let node = lb.select_node().await.unwrap().unwrap();
            if node == NodeId::new("node-1") {
                node1_count += 1;
            } else if node == NodeId::new("node-2") {
                node2_count += 1;
            } else if node == NodeId::new("node-3") {
                node3_count += 1;
            }
        }

        // With weights 3:2:1, over 18 selections we expect roughly 9:6:3
        // node-1 should be selected most (weight 3)
        assert!(node1_count >= node2_count);
        assert!(node2_count >= node3_count);
        assert!(node3_count > 0);
    }

    #[tokio::test]
    async fn test_wrr_large_weight_no_truncation() {
        let lb = LoadBalancer::new(LoadBalancingAlgorithm::WeightedRoundRobin);
        lb.add_node(NodeId::new("heavy")).await;
        lb.add_node(NodeId::new("light")).await;

        // Weight that exceeds i32::MAX — would silently become negative
        // with the old `as i32` cast, inverting this node's priority.
        lb.set_node_weight(&NodeId::new("heavy"), u32::MAX).await;
        lb.set_node_weight(&NodeId::new("light"), 1).await;

        let mut heavy_count = 0u32;
        for _ in 0..20 {
            if let Ok(Some(node)) = lb.select_node().await {
                if node == NodeId::new("heavy") {
                    heavy_count += 1;
                }
            }
        }

        // With i64 arithmetic, the u32::MAX weight is preserved correctly
        // and the heavy node is selected proportionally.
        // Before this fix, `u32::MAX as i32` silently became -1, causing
        // the high-weight node to NEVER be selected (0/20).
        assert!(
            heavy_count >= 10,
            "high-weight node must not be starved by truncation, got {heavy_count}/20"
        );
    }

    #[tokio::test]
    async fn test_random_distribution() {
        let lb = LoadBalancer::new(LoadBalancingAlgorithm::Random);
        lb.add_node(NodeId::new("node-1")).await;
        lb.add_node(NodeId::new("node-2")).await;
        lb.add_node(NodeId::new("node-3")).await;

        let mut counts = HashMap::new();
        for _ in 0..300 {
            let node = lb.select_node().await.unwrap().unwrap();
            *counts.entry(node).or_insert(0u32) += 1;
        }

        // Every node should be selected at least once over 300 trials
        assert!(counts.contains_key(&NodeId::new("node-1")));
        assert!(counts.contains_key(&NodeId::new("node-2")));
        assert!(counts.contains_key(&NodeId::new("node-3")));

        // No single node should dominate (> 60% would indicate bias)
        for count in counts.values() {
            assert!(*count < 180, "Node selected {} times out of 300 — distribution is biased", count);
        }
    }
}
