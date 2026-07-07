//! mofa-smith — MoFA's agent evaluation, scoring, and debugging CLI.
//!
//! The first component is `mofa smith eval run`, which runs a YAML or JSON
//! dataset of test cases through the swarm scheduler, scores each output, and
//! prints a pass/fail report.
//!
//! # Quick start
//!
//! ```bash
//! mofa-smith eval run --dataset examples/eval_basic.yaml
//! mofa-smith eval run --dataset examples/eval_basic.yaml --scorer keyword
//! mofa-smith eval run --dataset examples/eval_basic.yaml --output report.json
//! ```

mod cli;
mod eval;

use anyhow::Result;
use clap::Parser;
use mofa_foundation::swarm::CoordinationPattern;

use cli::{Cli, Commands, EvalCommands, PatternArg, ScorerArg};
use eval::{
    dataset::EvalDataset,
    report::{print_report, write_json_report},
    runner::EvalRunner,
    scorer::{ExactMatchScorer, KeywordScorer, LatencyScorer},
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Eval { action } => match action {
            EvalCommands::Run {
                dataset,
                scorer,
                pattern,
                timeout,
                pass_threshold,
                output,
                latency_target,
            } => {
                let ds = EvalDataset::load(&dataset)?;

                if ds.is_empty() {
                    anyhow::bail!("dataset is empty — add at least one case");
                }

                println!(
                    "[mofa-smith] loaded dataset: {} ({} cases)",
                    ds.name,
                    ds.len()
                );

                let coord_pattern = match pattern {
                    PatternArg::Sequential => CoordinationPattern::Sequential,
                    PatternArg::Parallel => CoordinationPattern::Parallel,
                };

                let runner = EvalRunner::new(
                    ds,
                    match scorer {
                        ScorerArg::Exact => Box::new(ExactMatchScorer),
                        ScorerArg::Keyword => Box::new(KeywordScorer),
                        ScorerArg::Latency => Box::new(LatencyScorer::new(latency_target)),
                    },
                )
                .with_pattern(coord_pattern)
                .with_timeout(timeout)
                .with_pass_threshold(pass_threshold);

                let report = runner.run().await;

                print_report(&report);

                if let Some(out_path) = output {
                    write_json_report(&report, &out_path)?;
                }

                if report.failed > 0 {
                    std::process::exit(1);
                }
            }
        },
    }

    Ok(())
}
