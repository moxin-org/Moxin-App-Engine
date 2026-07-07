use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use mofa_foundation::agent::components::tool::SimpleTool;
use mofa_foundation::orchestrator::ModelOrchestrator;
use mofa_kernel::agent::components::tool::ToolInput;
use mofa_kernel::bus::CommunicationMode;
use mofa_kernel::message::AgentMessage;
use mofa_testing::{
    BenchmarkCaseConfig, BenchmarkContext, BenchmarkRunner, BenchmarkThresholds, MetricThreshold,
    MockAgentBus, MockClock, MockLLMBackend, MockTool, ToolCallMetric,
};

fn benchmark_context() -> BenchmarkContext {
    let backend = Arc::new(MockLLMBackend::new());
    backend.add_response("weather", "sunny");

    let bus = Arc::new(MockAgentBus::new());

    let tool = Arc::new(MockTool::new(
        "search",
        "Searches documents",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        }),
    ));

    let mut tools = HashMap::new();
    tools.insert("search".to_string(), tool);

    BenchmarkContext::new(backend, bus, tools, "mock-model")
}

#[tokio::test]
async fn benchmark_runner_collects_metrics_and_exports_report() -> Result<()> {
    let report = BenchmarkRunner::new("agent benchmark suite")
        .with_clock(Arc::new(MockClock::starting_at(Duration::from_millis(
            1_710_000_000_000,
        ))))
        .run_case(
            BenchmarkCaseConfig::new("search flow", 3).with_warmup_iterations(1),
            benchmark_context,
            |context| async move {
                context
                    .backend
                    .infer(&context.model_name, "weather in paris")
                    .await?;

                let tool = context.tool("search").expect("tool should exist");
                let result = tool
                    .execute(ToolInput::from_json(serde_json::json!({
                        "query": "weather in paris"
                    })))
                    .await;
                assert!(result.success);

                let _ = context
                    .bus
                    .send_and_capture(
                        "planner",
                        CommunicationMode::Broadcast,
                        AgentMessage::TaskRequest {
                            task_id: "task-1".to_string(),
                            content: "weather in paris".to_string(),
                        },
                    )
                    .await;

                Ok(())
            },
        )
        .await?
        .build();

    assert_eq!(report.suite_name, "agent benchmark suite");
    assert_eq!(report.timestamp, 1_710_000_000_000);
    assert_eq!(report.total(), 1);
    assert_eq!(report.passed(), 1);

    let case = &report.cases[0];
    assert_eq!(case.name, "search flow");
    assert_eq!(case.iterations, 3);
    assert_eq!(case.warmup_iterations, 1);
    assert_eq!(case.total_infer_calls, 3);
    assert_eq!(case.infer_calls_per_iteration, 1);
    assert_eq!(case.total_bus_messages, 3);
    assert_eq!(case.bus_messages_per_iteration, 1);
    assert_eq!(case.tool_calls_per_iteration.get("search"), Some(&1));
    assert_eq!(case.sample_latencies_micros.len(), 3);
    assert!(case.regressions.is_empty());

    let json = serde_json::to_string(&report)?;
    assert!(json.contains("\"suite_name\":\"agent benchmark suite\""));
    assert!(json.contains("\"infer_calls_per_iteration\":1"));

    let test_report = report.to_test_report();
    assert_eq!(test_report.total(), 1);
    assert_eq!(test_report.passed(), 1);

    Ok(())
}

#[tokio::test]
async fn benchmark_runner_reports_regressions_when_thresholds_are_exceeded() -> Result<()> {
    let thresholds = BenchmarkThresholds {
        max_mean_latency_micros: None,
        max_peak_latency_micros: None,
        max_infer_calls_per_iteration: Some(MetricThreshold { max: 1 }),
        max_bus_messages_per_iteration: Some(MetricThreshold { max: 0 }),
        max_tool_calls_per_iteration: vec![ToolCallMetric {
            name: "search".to_string(),
            calls_per_iteration: 0,
        }],
    };

    let report = BenchmarkRunner::new("regression suite")
        .run_case(
            BenchmarkCaseConfig::new("chatty flow", 2).with_thresholds(thresholds),
            benchmark_context,
            |context| async move {
                context
                    .backend
                    .infer(&context.model_name, "weather")
                    .await?;
                context
                    .backend
                    .infer(&context.model_name, "weather tomorrow")
                    .await?;

                let tool = context.tool("search").expect("tool should exist");
                tool.execute(ToolInput::from_json(serde_json::json!({
                    "query": "weather"
                })))
                .await;

                let _ = context
                    .bus
                    .send_and_capture(
                        "planner",
                        CommunicationMode::Broadcast,
                        AgentMessage::TaskRequest {
                            task_id: "task-2".to_string(),
                            content: "weather".to_string(),
                        },
                    )
                    .await;

                Ok(())
            },
        )
        .await?
        .build();

    let case = &report.cases[0];
    assert_eq!(report.failed(), 1);
    assert_eq!(case.infer_calls_per_iteration, 2);
    assert_eq!(case.bus_messages_per_iteration, 1);
    assert_eq!(case.tool_calls_per_iteration.get("search"), Some(&1));
    assert_eq!(case.regressions.len(), 3);
    assert!(
        case.regressions
            .iter()
            .any(|item| item.contains("infer calls per iteration exceeded threshold"))
    );
    assert!(
        case.regressions
            .iter()
            .any(|item| item.contains("bus messages per iteration exceeded threshold"))
    );
    assert!(
        case.regressions
            .iter()
            .any(|item| item.contains("tool 'search' exceeded threshold"))
    );

    Ok(())
}
