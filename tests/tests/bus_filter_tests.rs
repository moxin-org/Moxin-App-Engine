use mofa_kernel::bus::CommunicationMode;
use mofa_kernel::message::AgentMessage;
use mofa_testing::bus::MockAgentBus;

#[tokio::test]
async fn messages_from_returns_only_from_sender() {
    let bus = MockAgentBus::new();
    let msg = AgentMessage::TaskRequest {
        task_id: "t1".into(),
        content: "c".into(),
    };
    
    let _ = bus.send_and_capture("a", CommunicationMode::Broadcast, msg.clone()).await;
    let _ = bus.send_and_capture("b", CommunicationMode::Broadcast, msg.clone()).await;
    let _ = bus.send_and_capture("a", CommunicationMode::Broadcast, msg.clone()).await;

    let from_a = bus.messages_from("a").await;
    assert_eq!(from_a.len(), 2);
    assert_eq!(from_a[0].0, "a");
    assert_eq!(from_a[1].0, "a");

    let from_b = bus.messages_from("b").await;
    assert_eq!(from_b.len(), 1);
    assert_eq!(from_b[0].0, "b");
}

#[tokio::test]
async fn messages_from_returns_empty_when_unknown() {
    let bus = MockAgentBus::new();
    let msg = AgentMessage::TaskRequest {
        task_id: "t1".into(),
        content: "c".into(),
    };
    
    let _ = bus.send_and_capture("a", CommunicationMode::Broadcast, msg).await;

    let from_c = bus.messages_from("c").await;
    assert_eq!(from_c.len(), 0);
}

#[tokio::test]
async fn messages_to_filters_by_recipient() {
    let bus = MockAgentBus::new();
    let msg = AgentMessage::TaskRequest {
        task_id: "t1".into(),
        content: "c".into(),
    };
    
    let _ = bus.send_and_capture("a", CommunicationMode::PointToPoint("target1".into()), msg.clone()).await;
    let _ = bus.send_and_capture("a", CommunicationMode::PointToPoint("target2".into()), msg.clone()).await;
    let _ = bus.send_and_capture("a", CommunicationMode::Broadcast, msg.clone()).await;

    let to_target1 = bus.messages_to("target1").await;
    assert_eq!(to_target1.len(), 1);
    
    if let CommunicationMode::PointToPoint(t) = &to_target1[0].1 {
        assert_eq!(t, "target1");
    } else {
        panic!("Expected PointToPoint");
    }

    let to_target2 = bus.messages_to("target2").await;
    assert_eq!(to_target2.len(), 1);
}

#[tokio::test]
async fn messages_to_returns_empty_when_no_messages() {
    let bus = MockAgentBus::new();
    let msg = AgentMessage::TaskRequest {
        task_id: "t1".into(),
        content: "c".into(),
    };
    
    let _ = bus.send_and_capture("a", CommunicationMode::Broadcast, msg.clone()).await;
    let _ = bus.send_and_capture("a", CommunicationMode::PointToPoint("target1".into()), msg).await;

    let to_unknown = bus.messages_to("unknown").await;
    assert_eq!(to_unknown.len(), 0);
}

#[tokio::test]
async fn messages_matching_predicate() {
    let bus = MockAgentBus::new();
    let msg1 = AgentMessage::TaskRequest {
        task_id: "t1".into(),
        content: "foo".into(),
    };
    let msg2 = AgentMessage::TaskRequest {
        task_id: "t2".into(),
        content: "bar".into(),
    };
    
    let _ = bus.send_and_capture("a", CommunicationMode::Broadcast, msg1).await;
    let _ = bus.send_and_capture("b", CommunicationMode::Broadcast, msg2).await;

    let matches = bus.messages_matching(|_, _, m| {
        if let AgentMessage::TaskRequest { task_id, .. } = m {
            task_id == "t2"
        } else {
            false
        }
    }).await;

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].0, "b");
}

#[tokio::test]
async fn messages_matching_returns_empty_when_no_match() {
    let bus = MockAgentBus::new();
    let msg1 = AgentMessage::TaskRequest {
        task_id: "t1".into(),
        content: "foo".into(),
    };
    
    let _ = bus.send_and_capture("a", CommunicationMode::Broadcast, msg1).await;

    let matches = bus.messages_matching(|_, _, m| {
        if let AgentMessage::TaskRequest { task_id, .. } = m {
            task_id == "t999"
        } else {
            false
        }
    }).await;

    assert_eq!(matches.len(), 0);
}

#[tokio::test]
async fn filters_work_after_fail_next_send() {
    let bus = MockAgentBus::new();
    bus.fail_next_send(1, "network error").await;

    let msg = AgentMessage::TaskRequest {
        task_id: "t1".into(),
        content: "foo".into(),
    };

    let res = bus.send_and_capture("a", CommunicationMode::PointToPoint("target1".into()), msg).await;
    assert!(res.is_err()); // Send failed

    // But should still be captured and filterable
    let from_a = bus.messages_from("a").await;
    assert_eq!(from_a.len(), 1);

    let to_target1 = bus.messages_to("target1").await;
    assert_eq!(to_target1.len(), 1);
}
