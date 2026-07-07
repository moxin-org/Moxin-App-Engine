use mofa_kernel::{
    DeliveryMode, DeliveryPolicy, MessageGraph, MessageGraphError, MessageNode,
    MessageNodeKind, MessageState, RouteRule, single_message_update, GraphState,
};
use mofa_kernel::message_graph::MessageEnvelope;
use serde_json::json;

fn build_order_routing_graph() -> Result<MessageGraph, Box<dyn std::error::Error>> {
    let mut graph = MessageGraph::new("order-routing").with_max_hops(8);

    graph.add_node(
        "orders_ingress",
        MessageNode::new(MessageNodeKind::Topic {
            topic: "orders.in".to_string(),
        }),
    )?;
    graph.add_node("router", MessageNode::new(MessageNodeKind::Router))?;
    graph.add_node(
        "fraud_agent",
        MessageNode::new(MessageNodeKind::Agent {
            agent_id: "fraud-checker".to_string(),
        }),
    )?;
    graph.add_node(
        "fulfillment_stream",
        MessageNode::new(MessageNodeKind::Stream {
            stream_id: "orders.fulfillment".to_string(),
        }),
    )?;
    graph.add_node(
        "orders_dlq",
        MessageNode::new(MessageNodeKind::Topic {
            topic: "orders.dlq".to_string(),
        }),
    )?;

    graph.add_entry_point("orders_ingress")?;
    graph.set_dead_letter_node("orders_dlq")?;

    graph.add_edge(
        "orders_ingress",
        "router",
        RouteRule::Always,
        DeliveryPolicy::default(),
    )?;

    graph.add_edge(
        "router",
        "fraud_agent",
        RouteRule::HeaderEquals {
            key: "risk".to_string(),
            value: "high".to_string(),
        },
        DeliveryPolicy {
            mode: DeliveryMode::Direct,
            ..DeliveryPolicy::default()
        },
    )?;

    graph.add_edge(
        "router",
        "fulfillment_stream",
        RouteRule::MessageType("order.created".to_string()),
        DeliveryPolicy {
            mode: DeliveryMode::PubSub,
            ..DeliveryPolicy::default()
        },
    )?;

    Ok(graph)
}

fn build_invalid_graph() -> MessageGraph {
    let mut graph = MessageGraph::new("invalid-routing");
    graph
        .add_node(
            "entry",
            MessageNode::new(MessageNodeKind::Topic {
                topic: "topic.in".to_string(),
            }),
        )
        .expect("static example node should be valid");
    graph
        .add_entry_point("entry")
        .expect("static example entry should be valid");
    graph
        .add_edge(
            "entry",
            "missing_target",
            RouteRule::Always,
            DeliveryPolicy::default(),
        )
        .expect("edge structure itself is valid");
    graph
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use case 1: practical route selection in an order pipeline.
    let compiled = build_order_routing_graph()?.compile()?;

    let high_risk_order =
        MessageEnvelope::new("order.created", br#"{"id":"A-100","amount":1200}"#.to_vec())
            .with_header("risk", "high");

    let route_targets = compiled
        .next_edges("router", &high_risk_order)?
        .iter()
        .map(|edge| edge.to.clone())
        .collect::<Vec<_>>();
    println!("High-risk order routes to: {:?}", route_targets);

    // Use case 2: pre-runtime validation catches malformed graph definitions.
    match build_invalid_graph().compile() {
        Ok(_) => println!("Unexpected: invalid graph compiled successfully"),
        Err(MessageGraphError::MissingNode(node)) => {
            println!("Validation blocked invalid graph, missing node: {node}");
        }
        Err(other) => println!("Validation failed with unexpected error: {other}"),
    }

    // Use case 3: StateGraph-style message state with `messages` key semantics.
    let mut state = MessageState::new().with_value("session_id", json!("sess-01"));
    state.push_message(high_risk_order.clone());
    let update = single_message_update(&MessageEnvelope::new(
        "order.acknowledged",
        br#"{"id":"A-100"}"#.to_vec(),
    ))?;
    futures::executor::block_on(state.apply_updates(&[update]))?;
    println!("MessageState count after updates: {}", state.messages().len());

    Ok(())
}
