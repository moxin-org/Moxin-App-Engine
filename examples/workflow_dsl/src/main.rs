//! Workflow DSL Example
//!
//! Demonstrates how to use the workflow DSL to define and execute workflows
//! using YAML configuration files through the mofa-sdk.

use mofa_sdk::llm::{LLMAgent, LLMAgentBuilder, MockLLMProvider};
use mofa_sdk::workflow::{
    ExecutorConfig, LlmAgentConfig, WorkflowDefinition, WorkflowDslParser,
    WorkflowExecutor, WorkflowValue,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Workflow DSL Example");

    // Run the customer support workflow example
    run_customer_support().await?;

    // Run the parallel agents workflow example
    run_parallel_agents().await?;

    Ok(())
}

/// Customer Support Workflow Example
async fn run_customer_support() -> Result<(), Box<dyn std::error::Error>> {
    info!("=== Customer Support Workflow ===");

    // Parse workflow from YAML file
    let definition = WorkflowDslParser::from_file("customer_support.yaml")?;
    info!(
        "Loaded workflow: {} - {}",
        definition.metadata.id,
        definition.metadata.name
    );

    // Build mock agents (in real usage, these would use actual LLM providers)
    let agent_registry = build_mock_agents(&definition).await?;

    // Build workflow from definition
    let workflow = WorkflowDslParser::build_with_agents(definition, &agent_registry).await?;
    info!("Built workflow with {} nodes", workflow.node_count());

    // Execute workflow
    let executor = WorkflowExecutor::new(ExecutorConfig::default());
    let input = WorkflowValue::String("I was charged twice for my subscription".to_string());

    info!("Executing workflow with input: {}", input.as_str().unwrap_or(""));
    let result = executor.execute(&workflow, input).await;

    info!("Workflow result: {:?}", result);

    Ok(())
}

/// Parallel Agents Workflow Example
async fn run_parallel_agents() -> Result<(), Box<dyn std::error::Error>> {
    info!("=== Parallel Agents Workflow ===");

    // Parse workflow from YAML file
    let definition = WorkflowDslParser::from_file("parallel_agents.yaml")?;
    info!(
        "Loaded workflow: {} - {}",
        definition.metadata.id,
        definition.metadata.name
    );

    // Build mock agents
    let agent_registry = build_mock_agents(&definition).await?;

    // Build workflow from definition
    let workflow = WorkflowDslParser::build_with_agents(definition, &agent_registry).await?;
    info!("Built workflow with {} nodes", workflow.node_count());

    // Execute workflow
    let executor = WorkflowExecutor::new(ExecutorConfig::default());
    let input = WorkflowValue::String(
        "The new product launch exceeded expectations with strong customer adoption."
            .to_string(),
    );

    info!("Executing workflow with input: {}", input.as_str().unwrap_or(""));
    let result = executor.execute(&workflow, input).await;

    info!("Workflow result: {:?}", result);

    Ok(())
}

/// Build mock agents from workflow definition
///
/// In a real application, you would build actual LLMAgent instances with
/// configured providers. This creates simple mock agents for demonstration.
async fn build_mock_agents(
    definition: &WorkflowDefinition,
) -> Result<HashMap<String, Arc<LLMAgent>>, Box<dyn std::error::Error>> {
    let mut registry = HashMap::new();

    // Check if we have an OpenAI API key
    let has_openai = std::env::var("OPENAI_API_KEY").is_ok();

    for (agent_id, config) in &definition.agents {
        let agent = if has_openai {
            // Build actual agent with OpenAI provider
            use mofa_sdk::llm::openai_from_env;

            let provider = Arc::new(openai_from_env()?);
            let mut builder = LLMAgentBuilder::new()
                .with_id(agent_id)
                .with_provider(provider);

            if let Some(prompt) = &config.system_prompt {
                builder = builder.with_system_prompt(prompt);
            }

            if let Some(temp) = config.temperature {
                builder = builder.with_temperature(temp);
            }

            if let Some(max_tokens) = config.max_tokens {
                builder = builder.with_max_tokens(max_tokens);
            }

            Arc::new(builder.build_async().await)
        } else {
            // Build mock agent without actual LLM
            build_mock_agent(agent_id, config).await?
        };

        registry.insert(agent_id.clone(), agent);
        info!("Registered agent: {}", agent_id);
    }

    Ok(registry)
}

/// Build a mock agent for demonstration
///
/// This creates a simple agent that returns predefined responses
/// based on the agent type. In production, use actual LLMAgent with
/// configured providers.
async fn build_mock_agent(
    agent_id: &str,
    config: &LlmAgentConfig,
) -> Result<Arc<LLMAgent>, Box<dyn std::error::Error>> {
    info!(
        "Building mock agent: {} with model: {}",
        agent_id,
        config.model
    );

    // Use a deterministic mock provider so the example can run in CI/offline.
    let provider = Arc::new(
        MockLLMProvider::new(&format!("mock-{}", agent_id))
            .with_default_response(format!("[{}] mock response", agent_id)),
    );

    let mut builder = LLMAgentBuilder::new()
        .with_id(agent_id)
        .with_name(&format!("Mock {}", agent_id))
        .with_provider(provider);

    if let Some(prompt) = &config.system_prompt {
        builder = builder.with_system_prompt(prompt);
    }

    Ok(Arc::new(builder.build_async().await))
}
