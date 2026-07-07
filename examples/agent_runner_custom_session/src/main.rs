use anyhow::Result;
use mofa_foundation::agent::context::prompt::AgentIdentity;
use mofa_testing::AgentTestRunner;

#[tokio::main]
async fn main() -> Result<()> {
    let mut runner = AgentTestRunner::new().await?;

    runner.write_bootstrap_file("CUSTOM.md", "Custom bootstrap content.")?;
    runner
        .configure_prompt(
            Some(AgentIdentity {
                name: "RunnerDemo".to_string(),
                description: "Custom identity for example runs".to_string(),
                icon: None,
            }),
            Some(vec!["CUSTOM.md".to_string()]),
        )
        .await;

    runner
        .mock_llm()
        .add_response("Custom session response")
        .await;

    let result = runner
        .run_text_with_session("demo-session", "hello session")
        .await?;

    println!(
        "Session id: {}",
        result
            .metadata
            .session_id
            .as_deref()
            .unwrap_or("<none>")
    );
    println!("Output: {}", result.output_text().unwrap_or_default());

    runner.shutdown().await?;
    Ok(())
}
