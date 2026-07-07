//! ReAct Agent 示例
//! ReAct Agent Example
//!
//! 演示如何使用 mofa-foundation 的 ReAct (Reasoning + Acting) 框架
//! Demonstrates how to use the mofa-foundation ReAct (Reasoning + Acting) framework
//!
//! # 运行方式
//! # How to run
//!
//! ```bash
//! # 设置 OpenAI API Key
//! # Set OpenAI API Key
//! export OPENAI_API_KEY=your-api-key
//!
//! # 可选: 设置自定义 API 端点 (如使用 Ollama 或其他兼容服务)
//! # Optional: Set custom API endpoint (e.g., using Ollama or other compatible services)
//! export OPENAI_BASE_URL=http://localhost:11434/v1
//!
//! # 运行示例
//! # Run example
//! cargo run -p react_agent
//! ```

use async_trait::async_trait;
use mofa_sdk::llm::{LLMAgent, LLMAgentBuilder, OpenAIConfig, OpenAIProvider};
use mofa_sdk::react::{
    prelude::*, spawn_react_actor, AutoAgent, ReActAgent, ReActConfig, ReActResult, ReActTool,
};
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

// ============================================================================
// 自定义工具实现
// Custom tool implementations
// ============================================================================

/// 网页搜索工具 (模拟)
/// Web search tool (mock)
struct WebSearchTool;

#[async_trait]
impl ReActTool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for information. Input should be a search query."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            },
            "required": ["query"]
        }))
    }

    async fn execute(&self, input: &str) -> Result<String, String> {
        // 解析输入
        // Parse input
        let query = if let Ok(json) = serde_json::from_str::<Value>(input) {
            json.get("query")
                .and_then(|v| v.as_str())
                .unwrap_or(input).to_owned()
        } else {
            input.to_owned()
        };

        info!("WebSearchTool: Searching for '{}'", query);

        // 模拟搜索结果
        // Mock search results
        let results = match query.to_lowercase().as_str() {
            q if q.contains("rust") && q.contains("language") => {
                r#"Search results for "Rust programming language":
1. Rust is a systems programming language focused on safety, concurrency, and performance.
2. Created by Mozilla Research, first stable release in 2015.
3. Key features: memory safety without garbage collection, zero-cost abstractions, fearless concurrency.
4. Used in Firefox, Dropbox, Cloudflare, and many other companies.
5. Voted "most loved language" in Stack Overflow survey for 8 consecutive years."#.to_owned()
            }
            q if q.contains("capital") && q.contains("france") => {
                r#"Search results for "capital of France":
1. Paris is the capital and largest city of France.
2. Population: approximately 2.1 million in the city proper, 12 million in metropolitan area.
3. Known for landmarks like Eiffel Tower, Louvre Museum, Notre-Dame Cathedral."#.to_owned()
            }
            q if q.contains("weather") => {
                r#"Search results for "weather":
Weather forecast services: Use a weather API for real-time data.
Current conditions vary by location. Please specify a city for accurate results."#.to_owned()
            }
            _ => {
                format!(
                    r#"Search results for "{query}":
Found 10 results. Here are the top 3:
1. General information about the topic
2. Related articles and resources
3. Recent news and updates"#
                )
            }
        };

        Ok(results.to_string())
    }
}

/// 天气查询工具 (模拟)
/// Weather query tool (mock)
struct WeatherTool;

#[async_trait]
impl ReActTool for WeatherTool {
    fn name(&self) -> &str {
        "weather"
    }

    fn description(&self) -> &str {
        "Get current weather information for a city. Input should be a city name."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string",
                    "description": "The city name"
                }
            },
            "required": ["city"]
        }))
    }

    async fn execute(&self, input: &str) -> Result<String, String> {
        let city = if let Ok(json) = serde_json::from_str::<Value>(input) {
            json.get("city")
                .and_then(|v| v.as_str())
                .unwrap_or(input).to_owned()
        } else {
            input.to_owned()
        };

        info!("WeatherTool: Getting weather for '{}'", city);

        // 模拟天气数据
        // Mock weather data
        let weather = match city.to_lowercase().as_str() {
            "paris" => "Paris: 18°C, Partly cloudy, Humidity: 65%, Wind: 12 km/h NW",
            "tokyo" => "Tokyo: 22°C, Sunny, Humidity: 55%, Wind: 8 km/h E",
            "new york" | "newyork" => {
                "New York: 15°C, Overcast, Humidity: 72%, Wind: 18 km/h SW"
            }
            "london" => "London: 12°C, Light rain, Humidity: 85%, Wind: 20 km/h W",
            "beijing" => "Beijing: 25°C, Clear, Humidity: 40%, Wind: 6 km/h N",
            _ => {
                return Ok(format!(
                    "{city}: 20°C, Clear skies, Humidity: 50%, Wind: 10 km/h (simulated data)"
                ));
            }
        };

        Ok(weather.to_owned())
    }
}

/// 维基百科查询工具 (模拟)
/// Wikipedia query tool (mock)
struct WikipediaTool;

#[async_trait]
impl ReActTool for WikipediaTool {
    fn name(&self) -> &str {
        "wikipedia"
    }

    fn description(&self) -> &str {
        "Look up information on Wikipedia. Input should be an article title or topic."
    }

    async fn execute(&self, input: &str) -> Result<String, String> {
        info!("WikipediaTool: Looking up '{}'", input);

        let article = match input.to_lowercase().as_str() {
            "rust" | "rust programming language" | "rust language" => {
                r#"**Rust (programming language)**

Rust is a multi-paradigm, high-level, general-purpose programming language that emphasizes
performance, type safety, and concurrency. It enforces memory safety without a garbage collector.

**History**: Development started at Mozilla Research in 2010. The first stable release, Rust 1.0,
was on May 15, 2015.

**Key Features**:
- Memory safety guarantees at compile time
- Zero-cost abstractions
- Minimal runtime
- Efficient C bindings
- Threads without data races

**Syntax**: Rust's syntax is similar to C and C++, with blocks of code delimited by curly brackets.

**Adoption**: Used in Firefox, Dropbox, Discord, and the Linux kernel."#.to_owned()
            }
            "paris" => {
                r#"**Paris**

Paris is the capital and most populous city of France, with an official population of 2,102,650.

**Location**: Northern France, on the River Seine
**Founded**: Around 250 BC by a Celtic tribe
**Landmarks**: Eiffel Tower, Louvre Museum, Arc de Triomphe, Notre-Dame Cathedral
**Economy**: Major global center for art, fashion, gastronomy, and culture"#.to_owned()
            }
            _ => {
                format!(
                    r#"**{input}**

No detailed article found. This topic may require more specific search terms.
Consider using web_search for more current information."#
                )
            }
        };

        Ok(article.to_string())
    }
}

// ============================================================================
// 示例函数
// Example functions
// ============================================================================

/// 示例 1: 基本 ReAct Agent 用法
/// Example 1: Basic ReAct Agent usage
async fn example_basic_react(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 1: Basic ReAct Agent");
    info!("{}\n", "=".repeat(60));

    // 创建 ReAct Agent
    // Create ReAct Agent
    let react_agent = ReActAgent::builder()
        .with_llm(llm_agent)
        .with_tool(Arc::new(WebSearchTool))
        .with_tool(Arc::new(WikipediaTool))
        .with_tool(calculator())
        .with_max_iterations(5)
        .with_temperature(0.7)
        .with_verbose(true)
        .build_async()
        .await?;

    // 执行任务
    // Execute task
    let task = "What is Rust programming language and when was it first released?";
    info!("Task: {}\n", task);

    let result = react_agent.run(task).await?;

    print_result(&result);

    Ok(())
}

/// 示例 2: 使用 Actor 模型
/// Example 2: Using the Actor model
async fn example_actor_model(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 2: ReAct Actor Model");
    info!("{}\n", "=".repeat(60));

    // 准备工具
    // Prepare tools
    let tools: Vec<Arc<dyn ReActTool>> = vec![
        Arc::new(WeatherTool),
        Arc::new(WebSearchTool),
        calculator(),
        datetime_tool(),
    ];

    // 启动 ReAct Actor
    // Start ReAct Actor
    let (actor_ref, _handle) = spawn_react_actor(
        "weather-react-agent",
        llm_agent,
        ReActConfig::default().with_max_iterations(5),
        tools,
    )
    .await?;

    // 获取状态
    // Get status
    let status = actor_ref.get_status().await?;
    info!("Actor Status: {:?}\n", status);

    // 执行任务
    // Execute task
    let task = "What is the current weather in Paris and Tokyo? Compare the temperatures.";
    info!("Task: {}\n", task);

    let result = actor_ref.run_task(task).await?;

    print_result(&result);

    // 停止 Actor
    // Stop Actor
    actor_ref.stop()?;

    Ok(())
}

/// 示例 3: AutoAgent - 自动选择策略
/// Example 3: AutoAgent - Automatic strategy selection
async fn example_auto_agent(
    llm_agent: Arc<LLMAgent>,
    react_agent: Arc<ReActAgent>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 3: AutoAgent - Automatic Strategy Selection");
    info!("{}\n", "=".repeat(60));

    // 创建 AutoAgent
    // Create AutoAgent
    let auto_agent = AutoAgent::new(llm_agent.clone(), react_agent).with_auto_mode(true);

    // 简单任务 - 应该使用 Direct 模式
    // Simple task - should use Direct mode
    let simple_task = "What is 2 + 2?";
    info!("Simple Task: {}", simple_task);
    let result = auto_agent.run(simple_task).await?;
    info!("Mode: {:?}", result.mode);
    info!("Answer: {}\n", result.answer);

    // 复杂任务 - 应该使用 ReAct 模式
    // Complex task - should use ReAct mode
    let complex_task = "Search for information about the Rust programming language and summarize its key features.";
    info!("Complex Task: {}", complex_task);
    let result = auto_agent.run(complex_task).await?;
    info!("Mode: {:?}", result.mode);
    info!("Answer: {}", result.answer);
    info!("Duration: {}ms\n", result.duration_ms);

    Ok(())
}

/// 示例 4: 使用内置工具
/// Example 4: Using built-in tools
async fn example_builtin_tools(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 4: Built-in Tools");
    info!("{}\n", "=".repeat(60));

    // 使用所有内置工具
    // Use all built-in tools
    let react_agent = ReActAgent::builder()
        .with_llm(llm_agent)
        .with_tools(all_builtin_tools()) // 计算器、字符串、JSON、日期时间、Echo
                                         // Calculator, String, JSON, DateTime, Echo
        .with_max_iterations(8)
        .build_async()
        .await?;

    // 数学计算任务
    // Math calculation task
    let task = "Calculate (25 * 4) + (100 / 5) and then get the current timestamp.";
    info!("Task: {}\n", task);

    let result = react_agent.run(task).await?;

    print_result(&result);

    Ok(())
}

/// 示例 5: 流式输出 (使用 Actor)
/// Example 5: Streaming output (using Actor)
async fn example_streaming(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 5: Streaming Output with Actor");
    info!("{}\n", "=".repeat(60));

    let tools: Vec<Arc<dyn ReActTool>> = vec![
        Arc::new(WikipediaTool),
        Arc::new(WebSearchTool),
    ];

    let (actor_ref, _handle) = spawn_react_actor(
        "streaming-react-agent",
        llm_agent,
        ReActConfig::default()
            .with_max_iterations(5)
            .with_stream_output(true),
        tools,
    )
    .await?;

    let task = "Look up Paris on Wikipedia and tell me about its famous landmarks.";
    info!("Task: {}\n", task);
    info!("Streaming steps:\n");

    // 使用流式 API
    // Use streaming API
    let (mut step_rx, result_rx) = actor_ref.run_task_streaming(task).await?;

    // 接收每个步骤
    // Receive each step
    while let Some(step) = step_rx.recv().await {
        info!(
            "[Step {}] {:?}: {}",
            step.step_number,
            step.step_type,
            &step.content[..step.content.len().min(100)]
        );
    }

    // 等待最终结果
    // Wait for final result
    let result = result_rx.await??;
    info!("\nFinal Answer: {}", result.answer);
    info!("Total iterations: {}", result.iterations);

    actor_ref.stop()?;

    Ok(())
}

/// 示例 6: 自定义工具组合
/// Example 6: Custom tool combination
async fn example_custom_tools(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 6: Custom Tool Combination");
    info!("{}\n", "=".repeat(60));

    // 组合自定义工具和内置工具
    // Combine custom and built-in tools
    let tools: Vec<Arc<dyn ReActTool>> = vec![
        Arc::new(WebSearchTool),
        Arc::new(WeatherTool),
        Arc::new(WikipediaTool),
        calculator(),
        datetime_tool(),
        string_tool(),
    ];

    let react_agent = ReActAgent::builder()
        .with_llm(llm_agent)
        .with_tools(tools)
        .with_max_iterations(10)
        .with_system_prompt(
            r#"You are a helpful research assistant.
You can search the web, check weather, look up Wikipedia articles, and perform calculations.
Always think step by step and use the appropriate tool for each subtask.
When you have gathered enough information, provide a comprehensive final answer."#,
        )
        .build_async()
        .await?;

    let task = "I'm planning a trip to Paris. Tell me about the weather there, some famous landmarks I should visit, and calculate how many days I would need if I want to spend 4 hours at each of 5 major attractions.";
    info!("Task: {}\n", task);

    let result = react_agent.run(task).await?;

    print_result(&result);

    Ok(())
}

/// 示例 7: Chain Agent (链式模式)
/// Example 7: Chain Agent (Sequential mode)
///
/// 多个 Agent 串行执行，前一个的输出作为后一个的输入
/// Multiple agents execute sequentially; output of the previous is input for the next.
async fn example_chain_agent(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 7: Chain Agent (Sequential Execution)");
    info!("{}\n", "=".repeat(60));

    // 创建研究者 Agent
    // Create Researcher Agent
    let researcher = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tool(Arc::new(WebSearchTool))
            .with_tool(Arc::new(WikipediaTool))
            .with_max_iterations(5)
            .with_system_prompt("You are a researcher. Search for information and provide detailed facts.")
            .build_async()
            .await?,
    );

    // 创建写作者 Agent
    // Create Writer Agent
    let writer = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tools(all_builtin_tools())
            .with_max_iterations(3)
            .with_system_prompt("You are a writer. Take the research provided and write a clear, engaging summary.")
            .build_async()
            .await?,
    );

    // 创建编辑者 Agent
    // Create Editor Agent
    let editor = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tool(string_tool())
            .with_max_iterations(3)
            .with_system_prompt("You are an editor. Review and polish the writing for clarity and style.")
            .build_async()
            .await?,
    );

    // 使用便捷函数创建链式 Agent
    // Create chain agent using utility function
    let chain = chain_agents(vec![
        ("researcher", researcher),
        ("writer", writer),
        ("editor", editor),
    ])
    .with_transform(|prev_output, next_name| {
        format!(
            "Previous step output:\n{prev_output}\n\nNow, as {next_name}, please continue with your task."
        )
    })
    .with_verbose(true);

    info!("Chain created with {} agents\n", chain.len());

    let task = "Research the Rust programming language and create a brief article about it.";
    info!("Initial Task: {}\n", task);

    let result = chain.run(task).await?;

    info!("\n--- Chain Result ---");
    info!("Success: {}", result.success);
    info!("Total Duration: {}ms", result.total_duration_ms);
    info!("Steps: {}\n", result.steps.len());

    for step in &result.steps {
        info!(
            "  Step {}: {} - {} ({}ms)",
            step.step,
            step.agent_name,
            if step.success { "Success" } else { "Failed" },
            step.output.duration_ms
        );
    }

    info!("\nFinal Output:\n{}", result.final_output);

    Ok(())
}

/// 示例 8: Parallel Agent (并行模式)
/// Example 8: Parallel Agent (Parallel mode)
///
/// 多个 Agent 并行执行同一任务，然后聚合结果
/// Multiple agents execute the same task in parallel, then aggregate results.
async fn example_parallel_agent(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 8: Parallel Agent (Concurrent Execution)");
    info!("{}\n", "=".repeat(60));

    // 创建多个专家 Agent
    // Create multiple expert agents
    let tech_expert = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tool(Arc::new(WebSearchTool))
            .with_max_iterations(3)
            .with_system_prompt("You are a technology expert. Analyze topics from a technical perspective.")
            .build_async()
            .await?,
    );

    let business_expert = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tool(Arc::new(WebSearchTool))
            .with_max_iterations(3)
            .with_system_prompt("You are a business analyst. Analyze topics from a business and market perspective.")
            .build_async()
            .await?,
    );

    let user_expert = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tool(Arc::new(WebSearchTool))
            .with_max_iterations(3)
            .with_system_prompt("You are a user experience expert. Analyze topics from an end-user perspective.")
            .build_async()
            .await?,
    );

    // 创建并行 Agent，使用拼接聚合
    // Create parallel agent using concatenation aggregation
    let parallel = ParallelAgent::new()
        .add("tech_expert", tech_expert)
        .add("business_expert", business_expert)
        .add("user_expert", user_expert)
        .with_aggregation(AggregationStrategy::Concatenate)
        .with_verbose(true);

    info!("Parallel Agent created with {} experts\n", parallel.len());

    let task = "Analyze the impact of the Rust programming language.";
    info!("Task: {}\n", task);

    let result = parallel.run(task).await?;

    info!("\n--- Parallel Result ---");
    info!("Success: {}", result.success);
    info!("Total Duration: {}ms", result.total_duration_ms);
    info!(
        "Results: {} succeeded, {} failed\n",
        result.success_count(),
        result.failure_count()
    );

    for individual in &result.individual_results {
        info!(
            "  {} - {} ({}ms)",
            individual.agent_name,
            if individual.success { "Success" } else { "Failed" },
            individual.output.duration_ms
        );
    }

    info!("\nAggregated Output:\n{}", result.aggregated_output);

    Ok(())
}

/// 示例 9: Parallel Agent with LLM Summarizer
/// Example 9: Parallel Agent with LLM Summarizer
///
/// 使用 LLM 聚合多个 Agent 的结果
/// Use LLM to aggregate results from multiple agents.
async fn example_parallel_with_summarizer(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 9: Parallel Agent with LLM Summarizer");
    info!("{}\n", "=".repeat(60));

    // 创建多个分析师 Agent
    // Create multiple analyst agents
    let analyst1 = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tool(Arc::new(WikipediaTool))
            .with_max_iterations(3)
            .with_system_prompt("You are Analyst 1. Provide your unique perspective on the topic.")
            .build_async()
            .await?,
    );

    let analyst2 = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tool(Arc::new(WebSearchTool))
            .with_max_iterations(3)
            .with_system_prompt("You are Analyst 2. Provide your unique perspective on the topic.")
            .build_async()
            .await?,
    );

    // 使用便捷函数创建带 LLM 聚合器的并行 Agent
    // Create parallel agent with LLM aggregator using utility function
    let parallel = parallel_agents_with_summarizer(
        vec![("analyst1", analyst1), ("analyst2", analyst2)],
        llm_agent.clone(),
    )
    .with_task_template("analyst1", "As Analyst 1, {task} Focus on historical context.")
    .with_task_template("analyst2", "As Analyst 2, {task} Focus on current trends.");

    info!("Parallel Agent with LLM Summarizer created\n");

    let task = "Discuss the evolution of programming languages.";
    info!("Task: {}\n", task);

    let result = parallel.run(task).await?;

    info!("\n--- Summarized Result ---");
    info!("Total Duration: {}ms\n", result.total_duration_ms);
    info!("LLM Synthesized Summary:\n{}", result.aggregated_output);

    Ok(())
}

/// 示例 10: MapReduce Agent
/// Example 10: MapReduce Agent
///
/// 将任务拆分、并行处理、然后归约结果
/// Split tasks, process in parallel, then reduce the results.
async fn example_map_reduce(llm_agent: Arc<LLMAgent>) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n{}", "=".repeat(60));
    info!("Example 10: MapReduce Agent");
    info!("{}\n", "=".repeat(60));

    // 创建工作 Agent
    // Create Worker Agent
    let worker = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_tools(all_builtin_tools())
            .with_max_iterations(3)
            .with_system_prompt("You are a worker. Process the given item and provide analysis.")
            .build_async()
            .await?,
    );

    // 创建归约 Agent
    // Create Reducer Agent
    let reducer = Arc::new(
        ReActAgent::builder()
            .with_llm(llm_agent.clone())
            .with_max_iterations(3)
            .with_system_prompt("You are a synthesizer. Combine multiple results into a coherent summary.")
            .build_async()
            .await?,
    );

    // 创建 MapReduce Agent
    // Create MapReduce Agent
    let map_reduce = MapReduceAgent::new()
        .with_mapper(|input| {
            // 按行拆分输入
            // Split input by lines
            input
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| format!("Analyze this item: {}", line.trim()))
                .collect()
        })
        .with_worker(worker)
        .with_reducer(reducer)
        .with_concurrency_limit(3)
        .with_verbose(true);

    let input = r#"
Rust programming language
Python programming language
JavaScript programming language
"#;

    info!("Input (3 items to analyze):\n{}", input);

    let result = map_reduce.run(input).await?;

    info!("\n--- MapReduce Result ---");
    info!("Total Duration: {}ms", result.total_duration_ms);
    info!("Map Results: {} items processed\n", result.map_results.len());

    for mr in &result.map_results {
        let status = if mr.output.is_some() {
            "Success"
        } else {
            "Failed"
        };
        info!("  Item {}: {}", mr.index + 1, status);
    }

    info!("\nReduced Output:\n{}", result.reduce_output.content);

    Ok(())
}

// ============================================================================
// 辅助函数
// Helper functions
// ============================================================================

fn print_result(result: &ReActResult) {
    info!("\n--- Result ---");
    info!("Success: {}", result.success);
    info!("Iterations: {}", result.iterations);
    info!("Duration: {}ms", result.duration_ms);
    info!("Steps taken: {}", result.steps.len());

    if !result.steps.is_empty() {
        info!("\nExecution trace:");
        for step in &result.steps {
            let content_preview = if step.content.len() > 80 {
                format!("{}...", &step.content[..80])
            } else {
                step.content.clone()
            };
            info!("  [{}] {:?}: {}", step.step_number, step.step_type, content_preview);
        }
    }

    info!("\nFinal Answer:\n{}", result.answer);

    if let Some(ref error) = result.error {
        info!("\nError: {}", error);
    }
}

fn create_llm_agent() -> Result<LLMAgent, Box<dyn std::error::Error>> {
    // 从环境变量获取配置
    // Get configuration from environment variables
    let api_key = std::env::var("OPENAI_API_KEY")
        .unwrap_or_else(|_| "demo-key".to_owned());

    let base_url = std::env::var("OPENAI_BASE_URL").ok();
    let model = std::env::var("OPENAI_MODEL")
        .unwrap_or_else(|_| "gpt-4".to_owned());

    let mut config = OpenAIConfig::new(api_key).with_model(&model);

    if let Some(url) = base_url {
        config = config.with_base_url(&url);
    }

    let provider = OpenAIProvider::with_config(config);

    let agent = LLMAgentBuilder::new()
        .with_name("ReAct Demo Agent")
        .with_provider(Arc::new(provider))
        .with_system_prompt("You are a helpful assistant that thinks step by step.")
        .with_temperature(0.7)
        .with_max_tokens(2048)
        .build();

    Ok(agent)
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    // 初始化日志
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("==========================================");
    info!("  MoFA ReAct Agent Examples");
    info!("==========================================");
    info!("\nThis example demonstrates the ReAct (Reasoning + Acting) pattern.");
    info!("The agent thinks step by step and uses tools to solve tasks.\n");

    // 创建 LLM Agent
    // Create LLM Agent
    let llm_agent = Arc::new(create_llm_agent()?);

    // 获取要运行的示例
    // Get the example to run
    let example = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "all".to_owned());

    match example.as_str() {
        "1" | "basic" => {
            example_basic_react(llm_agent).await?;
        }
        "2" | "actor" => {
            example_actor_model(llm_agent).await?;
        }
        "3" | "auto" => {
            // 需要先创建 react_agent
            // Need to create react_agent first
            let react_agent = Arc::new(
                ReActAgent::builder()
                    .with_llm(llm_agent.clone())
                    .with_tool(Arc::new(WebSearchTool))
                    .with_tool(Arc::new(WikipediaTool))
                    .with_tools(all_builtin_tools())
                    .with_max_iterations(5)
                    .build_async()
                    .await?,
            );
            example_auto_agent(llm_agent, react_agent).await?;
        }
        "4" | "builtin" => {
            example_builtin_tools(llm_agent).await?;
        }
        "5" | "streaming" => {
            example_streaming(llm_agent).await?;
        }
        "6" | "custom" => {
            example_custom_tools(llm_agent).await?;
        }
        "7" | "chain" => {
            example_chain_agent(llm_agent).await?;
        }
        "8" | "parallel" => {
            example_parallel_agent(llm_agent).await?;
        }
        "9" | "summarizer" => {
            example_parallel_with_summarizer(llm_agent).await?;
        }
        "10" | "mapreduce" => {
            example_map_reduce(llm_agent).await?;
        }
        "all" => {
            // 运行所有示例
            // Run all examples
            example_basic_react(llm_agent.clone()).await?;
            example_actor_model(llm_agent.clone()).await?;

            let react_agent = Arc::new(
                ReActAgent::builder()
                    .with_llm(llm_agent.clone())
                    .with_tool(Arc::new(WebSearchTool))
                    .with_tool(Arc::new(WikipediaTool))
                    .with_tools(all_builtin_tools())
                    .with_max_iterations(5)
                    .build_async()
                    .await?,
            );
            example_auto_agent(llm_agent.clone(), react_agent).await?;

            example_builtin_tools(llm_agent.clone()).await?;
            example_streaming(llm_agent.clone()).await?;
            example_custom_tools(llm_agent.clone()).await?;
            example_chain_agent(llm_agent.clone()).await?;
            example_parallel_agent(llm_agent.clone()).await?;
            example_parallel_with_summarizer(llm_agent.clone()).await?;
            example_map_reduce(llm_agent).await?;
        }
        _ => {
            info!("Unknown example: {}", example);
            info!("\nAvailable examples:");
            info!("  1, basic      - Basic ReAct Agent usage");
            info!("  2, actor      - ReAct Actor Model");
            info!("  3, auto       - AutoAgent with automatic strategy selection");
            info!("  4, builtin    - Using built-in tools");
            info!("  5, streaming  - Streaming output with Actor");
            info!("  6, custom     - Custom tool combination");
            info!("  7, chain      - Chain Agent (sequential execution)");
            info!("  8, parallel   - Parallel Agent (concurrent execution)");
            info!("  9, summarizer - Parallel Agent with LLM summarizer");
            info!("  10, mapreduce - MapReduce Agent pattern");
            info!("  all           - Run all examples");
        }
    }

    info!("\n==========================================");
    info!("  Examples completed!");
    info!("==========================================");

    Ok(())
}
