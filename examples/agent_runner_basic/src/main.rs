use anyhow::Result;
use mofa_testing::AgentTestRunner;

#[tokio::main]
async fn main() -> Result<()> {
    let mut runner = AgentTestRunner::new().await?;
    runner.mock_llm().add_response("Hello from the runner").await;

    let result = runner.run_text("hi").await?;
    println!("Output: {}", result.output_text().unwrap_or_default());
    println!(
        "Session: {}",
        result
            .metadata
            .session_id
            .as_deref()
            .unwrap_or("<none>")
    );
    println!("Workspace: {}", result.metadata.workspace_root.display());
    println!(
        "Runner stats: total={} success={} failed={}",
        result.metadata.runner_stats_after.total_executions,
        result.metadata.runner_stats_after.successful_executions,
        result.metadata.runner_stats_after.failed_executions
    );

    runner.shutdown().await?;
    Ok(())
}
