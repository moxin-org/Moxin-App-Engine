//! `mofa run` command implementation

use crate::CliError;
use colored::Colorize;

/// Execute the `mofa run` command
pub fn run(config: &std::path::Path, _dora: bool, dry_run: bool) -> Result<(), CliError> {
    if dry_run {
        return run_dry_run(config);
    }

    println!(
        "{} Running agent with config: {}",
        "→".green(),
        config.display()
    );

    let status = std::process::Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg("--config")
        .arg(config)
        .status()?;

    if !status.success() {
        println!("{} Agent exited with error", "✗".red());
        std::process::exit(1);
    }

    Ok(())
}

/// Validate a workflow definition without executing it
fn run_dry_run(config: &std::path::Path) -> Result<(), CliError> {
    use mofa_foundation::workflow::dsl::WorkflowDslParser;
    use mofa_foundation::workflow::validator::WorkflowValidator;
    use mofa_foundation::workflow::{WorkflowGraph, WorkflowNode, EdgeConfig};

    println!(
        "{} Dry-run: validating workflow {}",
        "→".cyan(),
        config.display()
    );

    // 1. Parse the DSL file
    let definition = WorkflowDslParser::from_file(config).map_err(|e| {
        CliError::Other(format!("Failed to parse workflow file: {}", e))
    })?;

    // 2. Build a lightweight WorkflowGraph from the definition
    //    We construct the graph manually from the parsed definition
    //    without requiring an agent registry (dry-run doesn't need real agents).
    let mut graph = WorkflowGraph::new(
        &definition.metadata.id,
        &definition.metadata.name,
    );

    for node_def in &definition.nodes {
        use mofa_foundation::workflow::dsl::NodeDefinition;
        match node_def {
            NodeDefinition::Start { id, .. } => {
                graph.add_node(WorkflowNode::start(id));
            }
            NodeDefinition::End { id, .. } => {
                graph.add_node(WorkflowNode::end(id));
            }
            NodeDefinition::Task { id, name, .. } => {
                graph.add_node(WorkflowNode::task(id, name, |_ctx, input| async move { Ok(input) }));
            }
            NodeDefinition::LlmAgent { id, name, .. } => {
                graph.add_node(WorkflowNode::task(id, name, |_ctx, input| async move { Ok(input) }));
            }
            NodeDefinition::Condition { id, name, .. } => {
                graph.add_node(WorkflowNode::task(id, name, |_ctx, input| async move { Ok(input) }));
            }
            NodeDefinition::Parallel { id, name, .. } => {
                graph.add_node(WorkflowNode::task(id, name, |_ctx, input| async move { Ok(input) }));
            }
            NodeDefinition::Join { id, name, wait_for, .. } => {
                let refs: Vec<&str> = wait_for.iter().map(|s| s.as_str()).collect();
                graph.add_node(WorkflowNode::join(id, name, refs));
            }
            NodeDefinition::Loop { id, name, .. } => {
                graph.add_node(WorkflowNode::task(id, name, |_ctx, input| async move { Ok(input) }));
            }
            NodeDefinition::Transform { id, name, .. } => {
                graph.add_node(WorkflowNode::task(id, name, |_ctx, input| async move { Ok(input) }));
            }
            NodeDefinition::SubWorkflow { id, name, workflow_id, .. } => {
                graph.add_node(WorkflowNode::sub_workflow(id, name, workflow_id));
            }
            NodeDefinition::Wait { id, name, event_type, .. } => {
                graph.add_node(WorkflowNode::wait(id, name, event_type));
            }
        }
    }

    for edge in &definition.edges {
        use mofa_foundation::workflow::EdgeConfig;
        if let Some(ref condition) = edge.condition {
            graph.add_edge(EdgeConfig::conditional(&edge.from, &edge.to, condition));
        } else {
            graph.add_edge(EdgeConfig::new(&edge.from, &edge.to));
        }
    }

    // 3. Run the validator
    let report = WorkflowValidator::validate(&graph);

    // 4. Print the report
    println!();
    println!(
        "  {} Graph structure: {} nodes, {} edges",
        if report.stats.start_nodes > 0 { "✓".green().to_string() } else { "✗".red().to_string() },
        report.stats.total_nodes,
        report.stats.total_edges,
    );

    println!(
        "  {} Start node present",
        if report.stats.start_nodes > 0 { "✓".green().to_string() } else { "✗".red().to_string() },
    );

    println!(
        "  {} End node(s) present",
        if report.stats.end_nodes > 0 { "✓".green().to_string() } else { "✗".red().to_string() },
    );

    let has_cycle_error = report.issues.iter().any(|i| {
        i.severity == mofa_foundation::workflow::Severity::Error && i.message.contains("cycle")
    });
    println!(
        "  {} No unintentional cycles",
        if !has_cycle_error { "✓".green().to_string() } else { "✗".red().to_string() },
    );

    // Print individual issues
    for issue in &report.issues {
        let icon = match issue.severity {
            mofa_foundation::workflow::Severity::Error => "✗".red().to_string(),
            mofa_foundation::workflow::Severity::Warning => "⚠".yellow().to_string(),
        };
        let node_ctx = issue.node_id.as_deref().unwrap_or("global");
        println!("  {} [{}] {}", icon, node_ctx, issue.message);
    }

    println!();
    if report.is_valid() {
        println!("{} Workflow validation passed!", "✓".green());
        Ok(())
    } else {
        let err_count = report.errors().count();
        println!(
            "{} Workflow validation failed with {} error(s).",
            "✗".red(),
            err_count
        );
        Err(CliError::Other("Workflow validation failed".to_string()))
    }
}

/// Execute the `mofa dataflow` command (requires dora feature)
#[cfg(feature = "dora")]
pub fn run_dataflow(file: &std::path::Path, uv: bool) -> Result<(), CliError> {
    use mofa_sdk::dora::{DoraRuntime, RuntimeConfig};

    println!("{} Running dataflow: {}", "→".green(), file.display());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let config = RuntimeConfig::embedded(file).with_uv(uv);
        let mut runtime = DoraRuntime::new(config);
        match runtime.run().await {
            Ok(result) => {
                println!("{} Dataflow {} completed", "✓".green(), result.uuid);
                Ok(())
            }
            Err(e) => Err(CliError::Other(format!("Dataflow failed: {}", e))),
        }
    })
}
