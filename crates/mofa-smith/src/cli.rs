//! CLI definitions for mofa-smith.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "mofa-smith",
    about = "MoFA Smith — agent evaluation, scoring, and debugging",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run evaluation commands.
    Eval {
        #[command(subcommand)]
        action: EvalCommands,
    },
}

#[derive(Subcommand)]
pub enum EvalCommands {
    /// Run a dataset through the swarm and score the results.
    Run {
        /// Path to the dataset file (.yaml or .json).
        #[arg(long, short = 'd')]
        dataset: PathBuf,

        /// Scorer to use.
        #[arg(long, short = 's', value_enum, default_value = "keyword")]
        scorer: ScorerArg,

        /// Coordination pattern for each eval case.
        #[arg(long, value_enum, default_value = "sequential")]
        pattern: PatternArg,

        /// Per-task timeout in seconds.
        #[arg(long, default_value = "30")]
        timeout: u64,

        /// Minimum score to count a case as passed (0.0 to 1.0).
        #[arg(long, default_value = "0.5")]
        pass_threshold: f64,

        /// Write the full report as JSON to this file.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Target wall time in seconds for the latency scorer.
        #[arg(long, default_value = "5.0")]
        latency_target: f64,
    },
}

#[derive(Clone, ValueEnum)]
pub enum ScorerArg {
    Exact,
    Keyword,
    Latency,
}

#[derive(Clone, ValueEnum)]
pub enum PatternArg {
    Sequential,
    Parallel,
}
