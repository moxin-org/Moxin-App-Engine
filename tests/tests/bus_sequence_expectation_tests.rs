use mofa_kernel::bus::CommunicationMode;
use mofa_kernel::message::AgentMessage;
use mofa_testing::bus::MockAgentBus;

fn task_message(task_id: &str, content: &str) -> AgentMessage {
    AgentMessage::TaskRequest {
        task_id: task_id.into(),
        content: content.into(),
    }
}

#[tokio::test]
async fn sender_sequence_matches_sent_order() {
    let bus = MockAgentBus::new();

    let _ = bus
        .send_and_capture(
            "agent-a",
            CommunicationMode::Broadcast,
            task_message("t1", "a"),
        )
        .await;
    let _ = bus
        .send_and_capture(
            "agent-b",
            CommunicationMode::PointToPoint("agent-c".into()),
            task_message("t2", "b"),
        )
        .await;

    assert!(bus.has_sender_sequence(&["agent-a", "agent-b"]).await);
    assert!(!bus.has_sender_sequence(&["agent-b", "agent-a"]).await);
}

#[tokio::test]
async fn mode_sequence_matches_sent_order() {
    let bus = MockAgentBus::new();

    let _ = bus
        .send_and_capture(
            "agent-a",
            CommunicationMode::Broadcast,
            task_message("t1", "a"),
        )
        .await;
    let _ = bus
        .send_and_capture(
            "agent-b",
            CommunicationMode::PointToPoint("agent-c".into()),
            task_message("t2", "b"),
        )
        .await;

    assert!(
        bus.has_mode_sequence(&[
            CommunicationMode::Broadcast,
            CommunicationMode::PointToPoint("agent-c".into()),
        ])
        .await
    );
    assert!(
        !bus.has_mode_sequence(&[
            CommunicationMode::PointToPoint("agent-c".into()),
            CommunicationMode::Broadcast,
        ])
        .await
    );
}

#[tokio::test]
async fn sender_mode_sequence_matches_pairs() {
    let bus = MockAgentBus::new();

    let _ = bus
        .send_and_capture(
            "agent-a",
            CommunicationMode::Broadcast,
            task_message("t1", "a"),
        )
        .await;
    let _ = bus
        .send_and_capture(
            "agent-b",
            CommunicationMode::PointToPoint("agent-c".into()),
            task_message("t2", "b"),
        )
        .await;

    assert!(
        bus.has_sender_mode_sequence(&[
            ("agent-a", CommunicationMode::Broadcast),
            ("agent-b", CommunicationMode::PointToPoint("agent-c".into()),),
        ])
        .await
    );
    assert!(
        !bus.has_sender_mode_sequence(&[
            ("agent-a", CommunicationMode::Broadcast),
            ("agent-b", CommunicationMode::Broadcast),
        ])
        .await
    );
}

#[tokio::test]
async fn sequence_helpers_require_exact_length() {
    let bus = MockAgentBus::new();

    let _ = bus
        .send_and_capture(
            "agent-a",
            CommunicationMode::Broadcast,
            task_message("t1", "a"),
        )
        .await;

    assert!(!bus.has_sender_sequence(&["agent-a", "agent-b"]).await);
    assert!(!bus.has_mode_sequence(&[]).await);
    assert!(
        !bus.has_sender_mode_sequence(&[
            ("agent-a", CommunicationMode::Broadcast),
            ("agent-b", CommunicationMode::Broadcast),
        ])
        .await
    );
}
