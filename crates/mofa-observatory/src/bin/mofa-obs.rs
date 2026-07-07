use axum::serve;
use clap::{Parser, Subcommand};
use mofa_observatory::{
    evaluation::{DatasetEntry, KeywordEvaluator, run_dataset},
    memory::episodic::Episode,
    tracing::Span,
};
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "mofa-obs", version, about = "Cognitive Observatory CLI")]
struct Cli {
    /// Observatory server base URL
    #[arg(long, default_value = "http://localhost:7070")]
    server: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Observatory server
    Server {
        #[arg(long, default_value = "7070")]
        port: u16,
        #[arg(long, default_value = "sqlite://observatory.db")]
        db: String,
    },
    /// Trace management
    Trace {
        #[command(subcommand)]
        action: TraceAction,
    },
    /// Evaluation management
    Eval {
        #[command(subcommand)]
        action: EvalAction,
    },
    /// Memory management
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
}

#[derive(Subcommand)]
enum TraceAction {
    /// Submit traces from a JSON file
    Submit {
        #[arg(long)]
        file: String,
    },
    /// List recent traces
    List {
        #[arg(long, default_value = "20")]
        limit: i64,
    },
}

#[derive(Subcommand)]
enum EvalAction {
    /// Run evaluators on a dataset
    Run {
        #[arg(long)]
        dataset: String,
        #[arg(long, help = "Comma-separated evaluator names: keyword,latency,llm_judge")]
        evaluators: String,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Add an episode to memory
    Add {
        #[arg(long)]
        session: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        content: String,
    },
    /// Search semantic memory
    Search {
        #[arg(long)]
        query: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Server { port, db } => {
            let storage = Arc::new(
                mofa_observatory::tracing::TraceStorage::new(&db).await?,
            );
            let episodic = Arc::new(
                mofa_observatory::memory::episodic::EpisodicMemory::new(&db).await?,
            );
            let state = mofa_observatory::api::routes::AppState { storage, episodic };
            let app = mofa_observatory::api::build_router(state);
            let addr = format!("0.0.0.0:{port}");
            println!("Observatory running on http://localhost:{port}");
            let listener = TcpListener::bind(&addr).await?;
            serve(listener, app).await?;
        }

        Commands::Trace { action } => {
            let client = reqwest::Client::new();
            match action {
                TraceAction::Submit { file } => {
                    let json = std::fs::read_to_string(&file)?;
                    let spans: Vec<Span> = serde_json::from_str(&json)?;
                    let resp = client
                        .post(format!("{}/v1/traces", cli.server))
                        .json(&spans)
                        .send()
                        .await?;
                    println!("Status: {}", resp.status());
                }
                TraceAction::List { limit } => {
                    let resp: serde_json::Value = client
                        .get(format!("{}/v1/traces?limit={limit}", cli.server))
                        .send()
                        .await?
                        .json()
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                }
            }
        }

        Commands::Eval { action } => match action {
            EvalAction::Run { dataset, evaluators } => {
                let content = std::fs::read_to_string(&dataset)?;
                let entries: Vec<DatasetEntry> = serde_json::from_str(&content)?;
                let eval_names: Vec<&str> = evaluators.split(',').map(str::trim).collect();

                let mut evals: Vec<Box<dyn mofa_observatory::evaluation::Evaluator>> = Vec::new();
                for name in eval_names {
                    match name {
                        "keyword" => evals.push(Box::new(KeywordEvaluator {
                            required_keywords: vec![],
                            forbidden_keywords: vec![],
                        })),
                        "latency" => evals.push(Box::new(
                            mofa_observatory::evaluation::LatencyEvaluator {
                                threshold_ms: 500,
                                measured_ms: 100,
                            },
                        )),
                        other => eprintln!("Unknown evaluator: {other} (supported: keyword, latency, llm_judge)"),
                    }
                }

                let results = run_dataset(&evals, &entries).await;
                println!("{}", serde_json::to_string_pretty(&results)?);
            }
        },

        Commands::Memory { action } => {
            let client = reqwest::Client::new();
            match action {
                MemoryAction::Add { session, role, content } => {
                    let ep = Episode {
                        id: uuid::Uuid::new_v4(),
                        session_id: session,
                        timestamp: chrono::Utc::now(),
                        role,
                        content,
                        metadata: Default::default(),
                    };
                    let resp = client
                        .post(format!("{}/v1/memory/episodes", cli.server))
                        .json(&ep)
                        .send()
                        .await?;
                    println!("Status: {}", resp.status());
                }
                MemoryAction::Search { query } => {
                    let resp: serde_json::Value = client
                        .get(format!("{}/v1/memory/search", cli.server))
                        .query(&[("q", &query)])
                        .send()
                        .await?
                        .json()
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                }
            }
        }
    }
    Ok(())
}
