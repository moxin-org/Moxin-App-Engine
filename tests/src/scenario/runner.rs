use crate::backend::MockLLMBackend;
use crate::bus::MockAgentBus;
use crate::clock::MockClock;
use crate::report::{TestCaseResult, TestReport, TestReportBuilder, TestStatus};
use crate::scenario::spec::{ScenarioSpec, ToolResultSpec};
use crate::tools::MockTool;
use mofa_foundation::orchestrator::{
    ModelOrchestrator, ModelProviderConfig, ModelType, OrchestratorError,
};
use mofa_kernel::agent::components::tool::ToolResult;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ScenarioContext {
    pub backend: Arc<MockLLMBackend>,
    pub bus: Arc<MockAgentBus>,
    pub clock: Arc<MockClock>,
    pub model_name: String,
    tools: HashMap<String, MockTool>,
    tracked_prompts: Arc<RwLock<Vec<String>>>,
}

impl ScenarioContext {
    pub async fn infer(&self, prompt: &str) -> Result<String, OrchestratorError> {
        self.tracked_prompts.write().await.push(prompt.to_string());
        self.backend.infer(&self.model_name, prompt).await
    }

    pub fn tool(&self, name: &str) -> Option<MockTool> {
        self.tools.get(name).cloned()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

pub struct ScenarioRunner {
    spec: ScenarioSpec,
}

impl ScenarioRunner {
    pub fn new(spec: ScenarioSpec) -> Self {
        Self { spec }
    }

    pub async fn run<F, Fut>(&self, scenario: F) -> anyhow::Result<TestReport>
    where
        F: FnOnce(ScenarioContext) -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        let context = self.build_context().await?;

        let mut builder = TestReportBuilder::new(self.spec.suite_name.clone())
            .with_clock(context.clock.clone())
            .record("scenario_execution", || async {
                scenario(context.clone()).await
            })
            .await;

        for result in self.evaluate_expectations(&context).await {
            builder = builder.add_result(result);
        }

        Ok(builder.build())
    }

    async fn build_context(&self) -> anyhow::Result<ScenarioContext> {
        let mut backend = MockLLMBackend::new();
        let llm_spec = &self.spec.llm;
        let model_name = llm_spec
            .model_name
            .clone()
            .unwrap_or_else(|| "scenario-model".to_string());

        if let Some(fallback) = &llm_spec.fallback {
            backend.set_fallback(fallback);
        }

        for rule in &llm_spec.responses {
            backend.add_response(&rule.prompt_substring, &rule.response);
        }

        for sequence in &llm_spec.response_sequences {
            let responses = sequence.responses.iter().map(String::as_str).collect();
            backend.add_response_sequence(&sequence.prompt_substring, responses);
        }

        for failure in &llm_spec.fail_next {
            backend.fail_next(
                failure.count,
                OrchestratorError::InferenceFailed(failure.error.clone()),
            );
        }

        for failure in &llm_spec.fail_on {
            backend.fail_on(
                &failure.prompt_substring,
                OrchestratorError::Other(failure.error.clone()),
            );
        }

        backend
            .register_model(make_model_config(&model_name))
            .await
            .map_err(anyhow::Error::from)?;
        backend
            .load_model(&model_name)
            .await
            .map_err(anyhow::Error::from)?;

        let clock = Arc::new(match self.spec.clock.start_ms {
            Some(ms) => MockClock::starting_at(Duration::from_millis(ms)),
            None => MockClock::new(),
        });
        if let Some(step) = self.spec.clock.auto_advance_ms {
            clock.set_auto_advance(Duration::from_millis(step));
        }

        let bus = Arc::new(MockAgentBus::new());
        for failure in &self.spec.bus.fail_next_send {
            bus.fail_next_send(failure.count, &failure.error).await;
        }

        let mut tools = HashMap::new();
        for tool_spec in &self.spec.tools {
            let tool = MockTool::new(
                &tool_spec.name,
                &tool_spec.description,
                tool_spec.schema.clone(),
            );

            if let Some(result) = &tool_spec.stubbed_result {
                tool.set_result(to_tool_result(result)).await;
            }

            for failure in &tool_spec.fail_next {
                tool.fail_next(failure.count, &failure.error).await;
            }

            for failure in &tool_spec.fail_on_input {
                tool.fail_on_input(failure.input.clone(), &failure.error)
                    .await;
            }

            if !tool_spec.result_sequence.is_empty() {
                let sequence = tool_spec
                    .result_sequence
                    .iter()
                    .map(to_tool_result)
                    .collect();
                tool.add_result_sequence(sequence).await;
            }

            tools.insert(tool_spec.name.clone(), tool);
        }

        Ok(ScenarioContext {
            backend: Arc::new(backend),
            bus,
            clock,
            model_name,
            tools,
            tracked_prompts: Arc::new(RwLock::new(Vec::new())),
        })
    }

    async fn evaluate_expectations(&self, context: &ScenarioContext) -> Vec<TestCaseResult> {
        let mut results = Vec::new();

        if let Some(expected) = self.spec.expectations.infer_total {
            let actual = context.backend.call_count();
            results.push(expectation_result(
                "expect_infer_total",
                actual == expected,
                format!("expected infer_total={expected}, got {actual}"),
            ));
        }

        for expectation in &self.spec.expectations.prompt_counts {
            let actual = {
                let prompts = context.tracked_prompts.read().await;
                prompts
                    .iter()
                    .filter(|prompt| prompt.contains(&expectation.substring))
                    .count()
            };
            results.push(expectation_result(
                &format!("expect_prompt_count::{}", expectation.substring),
                actual == expectation.expected,
                format!(
                    "expected prompt substring '{}' count={}, got {}",
                    expectation.substring, expectation.expected, actual
                ),
            ));
        }

        for expectation in &self.spec.expectations.tool_calls {
            let result = match context.tool(&expectation.tool_name) {
                Some(tool) => {
                    let actual = tool.call_count().await;
                    expectation_result(
                        &format!("expect_tool_calls::{}", expectation.tool_name),
                        actual == expectation.expected,
                        format!(
                            "expected tool '{}' calls={}, got {}",
                            expectation.tool_name, expectation.expected, actual
                        ),
                    )
                }
                None => expectation_result(
                    &format!("expect_tool_calls::{}", expectation.tool_name),
                    false,
                    format!(
                        "tool '{}' not found in scenario context",
                        expectation.tool_name
                    ),
                ),
            };
            results.push(result);
        }

        for expectation in &self.spec.expectations.bus_messages_from {
            let actual = {
                let messages = context.bus.captured_messages.read().await;
                messages
                    .iter()
                    .filter(|(sender, _, _)| sender == &expectation.sender_id)
                    .count()
            };
            results.push(expectation_result(
                &format!("expect_bus_messages_from::{}", expectation.sender_id),
                actual == expectation.expected,
                format!(
                    "expected sender '{}' messages={}, got {}",
                    expectation.sender_id, expectation.expected, actual
                ),
            ));
        }

        results
    }
}

fn make_model_config(name: &str) -> ModelProviderConfig {
    ModelProviderConfig {
        model_name: name.into(),
        model_path: "/mock".into(),
        device: "cpu".into(),
        model_type: ModelType::Llm,
        max_context_length: None,
        quantization: None,
        extra_config: HashMap::new(),
    }
}

fn to_tool_result(spec: &ToolResultSpec) -> ToolResult {
    let mut result = if let Some(error) = &spec.error {
        ToolResult::failure(error)
    } else if let Some(json) = &spec.json {
        ToolResult::success(json.clone())
    } else if let Some(text) = &spec.text {
        ToolResult::success_text(text)
    } else {
        ToolResult::success_text("Mock execution default")
    };

    for (key, value) in &spec.metadata {
        result.metadata.insert(key.clone(), value.clone());
    }

    result
}

fn expectation_result(name: &str, passed: bool, failure_message: String) -> TestCaseResult {
    TestCaseResult {
        name: name.to_string(),
        status: if passed {
            TestStatus::Passed
        } else {
            TestStatus::Failed
        },
        duration: Duration::ZERO,
        error: if passed { None } else { Some(failure_message) },
        metadata: Vec::new(),
    }
}
