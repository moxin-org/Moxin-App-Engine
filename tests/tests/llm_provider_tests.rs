use mofa_kernel::agent::AgentError;
use mofa_kernel::llm::provider::LLMProvider;
use mofa_kernel::llm::types::*;
use mofa_testing::MockLLMProvider;

fn make_request(user_msg: &str) -> ChatCompletionRequest {
    ChatCompletionRequest::new("mock-model").user(user_msg)
}

#[tokio::test]
async fn default_fallback_response() {
    let mock = MockLLMProvider::new();
    let resp = mock.chat(make_request("anything")).await.unwrap();
    assert_eq!(resp.content(), Some("Mock response."));
}

#[tokio::test]
async fn preset_response_matching() {
    let mock = MockLLMProvider::new();
    mock.add_response("weather", "It is sunny.");
    mock.add_response("time", "It is noon.");

    let resp = mock.chat(make_request("what is the weather?")).await.unwrap();
    assert_eq!(resp.content(), Some("It is sunny."));

    let resp = mock.chat(make_request("tell me the time")).await.unwrap();
    assert_eq!(resp.content(), Some("It is noon."));
}

#[tokio::test]
async fn multiple_rules_first_match_wins() {
    let mock = MockLLMProvider::new();
    mock.add_response("hello", "first");
    mock.add_response("hello", "second");

    let resp = mock.chat(make_request("hello")).await.unwrap();
    assert_eq!(resp.content(), Some("first"));
}

#[tokio::test]
async fn no_match_returns_fallback() {
    let mut mock = MockLLMProvider::new();
    mock.set_fallback("custom fallback");
    mock.add_response("xyz", "won't match");

    let resp = mock.chat(make_request("hello")).await.unwrap();
    assert_eq!(resp.content(), Some("custom fallback"));
}

#[tokio::test]
async fn tool_call_response() {
    let mock = MockLLMProvider::new();
    let tool_call = ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "get_weather".into(),
            arguments: r#"{"city":"NYC"}"#.into(),
        },
    };
    mock.add_tool_call_response("weather", vec![tool_call]);

    let resp = mock.chat(make_request("check weather")).await.unwrap();
    assert!(resp.has_tool_calls());
    assert_eq!(resp.tool_calls().unwrap()[0].function.name, "get_weather");
    assert_eq!(resp.finish_reason(), Some(&FinishReason::ToolCalls));
}

#[tokio::test]
async fn no_tools_returns_text() {
    let mock = MockLLMProvider::new();
    mock.add_response("hello", "hi there");

    let resp = mock.chat(make_request("hello")).await.unwrap();
    assert!(!resp.has_tool_calls());
    assert_eq!(resp.content(), Some("hi there"));
}

#[tokio::test]
async fn response_sequence() {
    let mock = MockLLMProvider::new();
    mock.add_response_sequence("count", vec!["one", "two", "three"]);

    let r1 = mock.chat(make_request("count")).await.unwrap();
    assert_eq!(r1.content(), Some("one"));

    let r2 = mock.chat(make_request("count")).await.unwrap();
    assert_eq!(r2.content(), Some("two"));

    // Last value repeats
    let r3 = mock.chat(make_request("count")).await.unwrap();
    assert_eq!(r3.content(), Some("three"));

    let r4 = mock.chat(make_request("count")).await.unwrap();
    assert_eq!(r4.content(), Some("three"));
}

#[tokio::test]
async fn failure_injection() {
    let mock = MockLLMProvider::new();
    mock.fail_next(AgentError::Other("test error".into()));

    let result = mock.chat(make_request("hello")).await;
    assert!(result.is_err());

    // Next call succeeds
    let result = mock.chat(make_request("hello")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn request_history_recorded() {
    let mock = MockLLMProvider::new();
    assert!(mock.request_history().is_empty());

    mock.chat(make_request("first")).await.unwrap();
    mock.chat(make_request("second")).await.unwrap();

    let history = mock.request_history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].messages[0].text_content(), Some("first"));
    assert_eq!(history[1].messages[0].text_content(), Some("second"));
}

#[tokio::test]
async fn call_count_increments() {
    let mock = MockLLMProvider::new();
    assert_eq!(mock.call_count(), 0);

    mock.chat(make_request("a")).await.unwrap();
    mock.chat(make_request("b")).await.unwrap();
    mock.chat(make_request("c")).await.unwrap();

    assert_eq!(mock.call_count(), 3);
}

#[tokio::test]
async fn call_count_increments_on_failure() {
    let mock = MockLLMProvider::new();
    mock.fail_next(AgentError::Other("err".into()));

    let _ = mock.chat(make_request("a")).await;
    assert_eq!(mock.call_count(), 1);
}

#[tokio::test]
async fn last_request_returns_most_recent() {
    let mock = MockLLMProvider::new();
    assert!(mock.last_request().is_none());

    mock.chat(make_request("first")).await.unwrap();
    mock.chat(make_request("second")).await.unwrap();

    let last = mock.last_request().unwrap();
    assert_eq!(last.messages[0].text_content(), Some("second"));
}

#[tokio::test]
async fn reset_clears_state() {
    let mock = MockLLMProvider::new();
    mock.fail_next(AgentError::Other("err".into()));
    mock.chat(make_request("a")).await.ok();

    assert_eq!(mock.call_count(), 1);
    assert_eq!(mock.request_history().len(), 1);

    mock.reset();

    assert_eq!(mock.call_count(), 0);
    assert!(mock.request_history().is_empty());
}

#[tokio::test]
async fn embedding_returns_data() {
    let mock = MockLLMProvider::new();
    let req = EmbeddingRequest {
        model: "mock-model".into(),
        input: EmbeddingInput::Single("hello".into()),
    };
    let resp = mock.embedding(req).await.unwrap();
    assert_eq!(resp.data.len(), 1);
    assert_eq!(resp.data[0].embedding.len(), 3);
    assert!(resp.usage.is_some());
}

#[tokio::test]
async fn health_check_returns_true() {
    let mock = MockLLMProvider::new();
    assert!(mock.health_check().await.unwrap());
}

#[tokio::test]
async fn trait_metadata() {
    let mock = MockLLMProvider::new();
    assert_eq!(mock.name(), "MockLLMProvider");
    assert_eq!(mock.default_model(), "mock-model");
    assert!(mock.supports_tools());
    assert!(!mock.supports_streaming());
}
