#![allow(missing_docs)]

use mofa_sdk::llm::{LLMAgent, LLMAgentBuilder, MockLLMProvider};
use mofa_sdk::workflow::{
    ExecutorConfig, WorkflowDefinition, WorkflowDslParser, WorkflowExecutor, WorkflowValue,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct BaselineFile {
    supported_dsl_versions: Vec<String>,
    cases: Vec<BaselineCase>,
}

#[derive(Debug, Deserialize)]
struct BaselineCase {
    id: String,
    fixture: String,
    expected_version: String,
    expected_node_count: usize,
    expected_edge_count: usize,
    expected_output_markers: Vec<String>,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("resolve workspace root")
}

fn load_baselines() -> BaselineFile {
    let root = workspace_root();
    let path = root
        .join("examples")
        .join("cli_production_smoke")
        .join("fixtures")
        .join("workflow_dsl")
        .join("baselines.json");

    let content = fs::read_to_string(&path).expect("read DSL baselines file");
    serde_json::from_str(&content).expect("parse DSL baselines file")
}

async fn build_mock_agents(definition: &WorkflowDefinition) -> HashMap<String, Arc<LLMAgent>> {
    let mut registry = HashMap::new();

    for (agent_id, config) in &definition.agents {
        let provider = Arc::new(
            MockLLMProvider::new(&format!("mock-{}", agent_id))
                .with_default_response(format!("[{}] mock response", agent_id)),
        );

        let mut builder = LLMAgentBuilder::new().with_id(agent_id).with_provider(provider);

        if let Some(prompt) = &config.system_prompt {
            builder = builder.with_system_prompt(prompt);
        }

        if let Some(temp) = config.temperature {
            builder = builder.with_temperature(temp);
        }

        if let Some(max_tokens) = config.max_tokens {
            builder = builder.with_max_tokens(max_tokens);
        }

        registry.insert(agent_id.clone(), Arc::new(builder.build_async().await));
    }

    registry
}

#[tokio::test]
async fn workflow_dsl_fixtures_parse_build_execute_regression() {
    let root = workspace_root();
    let baselines = load_baselines();
    let supported_versions = baselines.supported_dsl_versions.clone();

    for case in baselines.cases {
        let fixture_path = root.join("examples").join("workflow_dsl").join(&case.fixture);

        let definition = WorkflowDslParser::from_file(&fixture_path)
            .unwrap_or_else(|e| panic!("{}: parse failed: {}", case.id, e));

        let version = definition
            .metadata
            .version
            .as_deref()
            .unwrap_or("<missing>")
            .to_string();

        assert_eq!(
            version, case.expected_version,
            "{}: fixture version drifted; update fixture baseline explicitly",
            case.id
        );

        assert!(
            supported_versions.iter().any(|v| v == &version),
            "{}: version {} is unsupported by regression policy; update baselines intentionally",
            case.id,
            version
        );

        let registry = build_mock_agents(&definition).await;

        let workflow = WorkflowDslParser::build_with_agents(definition, &registry)
            .await
            .unwrap_or_else(|e| panic!("{}: build failed: {}", case.id, e));

        assert_eq!(
            workflow.node_count(),
            case.expected_node_count,
            "{}: node_count changed; update baseline intentionally",
            case.id
        );
        assert_eq!(
            workflow.edge_count(),
            case.expected_edge_count,
            "{}: edge_count changed; update baseline intentionally",
            case.id
        );

        workflow
            .validate()
            .unwrap_or_else(|errs| panic!("{}: graph validation failed: {:?}", case.id, errs));

        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let input = WorkflowValue::String("regression input".to_string());

        let execution = executor
            .execute(&workflow, input)
            .await
            .unwrap_or_else(|e| panic!("{}: execute failed: {}", case.id, e));

        let snapshot = format!("{:?}", execution);
        for marker in &case.expected_output_markers {
            assert!(
                snapshot.contains(marker),
                "{}: output marker '{}' missing; update baseline intentionally\n{}",
                case.id,
                marker,
                snapshot
            );
        }
    }
}
