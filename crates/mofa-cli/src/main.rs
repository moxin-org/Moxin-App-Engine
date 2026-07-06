//! MoFA CLI - Command-line tool for building and managing AI agents

mod cli;
mod commands;
mod config;
mod context;
mod output;
mod plugin_catalog;
mod render;
mod state;
mod store;
mod tui;
mod utils;
mod widgets;

mod error;
pub use error::{CliError, CliResult, IntoCliReport};

use clap::Parser;
use cli::Cli;
use colored::Colorize;
use context::CliContext;
use error_stack::ResultExt as _;

fn main() {
    // Install the global error-stack hooks FIRST so every Report rendered
    // afterward benefits from the configured debug output.
    error::install_hook();

    let mut args: Vec<String> = std::env::args().collect();
    normalize_legacy_output_flags(&mut args);
    let cli = Cli::parse_from(args);

    if cli.output_legacy.is_some() {
        eprintln!("Warning: '--output' is deprecated. Use '--output-format' instead.");
    }

    // Initialize logging.
    if cli.verbose {
        tracing_subscriber::fmt().with_env_filter("debug").init();
    } else {
        tracing_subscriber::fmt().with_env_filter("info").init();
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let report = error_stack::Report::new(CliError::Io(e))
                .attach("failed to initialize the async runtime");
            eprintln!("{report:?}");
            std::process::exit(1);
        }
    };

    // Launch TUI if requested or no command provided.
    let result: CliResult<()> = if cli.tui || cli.command.is_none() {
        rt.block_on(async {
            tui::run()
                .await
                .into_report()
                .attach("running TUI mode")
                .map(|_| ())
        })
    } else {
        rt.block_on(run_command(cli))
    };

    if let Err(report) = result {
        // Print the full error chain to stderr, then exit non-zero.
        // Set RUST_BACKTRACE=1 for source locations in each frame.
        eprintln!("{report:?}");
        std::process::exit(1);
    }
}

async fn run_command(cli: Cli) -> CliResult<()> {
    use cli::Commands;

    // Initialize context for commands that need backend services
    let needs_context = matches!(
        &cli.command,
        Some(
            Commands::Agent(_)
                | Commands::Plugin { .. }
                | Commands::Session { .. }
                | Commands::Tool { .. }
        )
    );

    let ctx = if needs_context {
        Some(
            CliContext::new()
                .await
                .into_report()
                .attach("initializing CLI context (runtime, registry, kernel)")?,
        )
    } else {
        None
    };

    match cli.command {
        Some(Commands::New {
            name,
            template,
            output,
        }) => {
            commands::new::run(&name, &template, output.as_deref())
                .into_report()
                .attach_with(|| {
                    format!("scaffolding project '{name}' with template '{template}'")
                })?;
        }

        Some(Commands::Init { path }) => {
            commands::init::run(&path)?;
        }

        Some(Commands::Build { release, features }) => {
            commands::build::run(release, features.as_deref())?;
        }

        Some(Commands::Run { config, dora, dry_run }) => {
            commands::run::run(&config, dora, dry_run)?;
        }

        #[cfg(feature = "dora")]
        Some(Commands::Dataflow { file, uv }) => {
            commands::run::run_dataflow(&file, uv)?;
        }

        Some(Commands::Generate { what }) => match what {
            cli::GenerateCommands::Config { output } => {
                commands::generate::run_config(&output)?;
            }
            cli::GenerateCommands::Dataflow { output } => {
                commands::generate::run_dataflow(&output)?;
            }
        },

        Some(Commands::Info) => {
            commands::generate::run_info();
        }

        Some(Commands::Doctor {
            path,
            scenario,
            json,
            fix,
            strict,
        }) => {
            commands::doctor::run(Some(path), scenario, strict, json, fix)
                .map_err(|e| CliError::Other(e.to_string()))?;
        }

        Some(Commands::Db { action }) => match action {
            cli::DbCommands::Init {
                db_type,
                output,
                database_url,
            } => {
                commands::db::run_init(db_type, output, database_url)?;
            }
            cli::DbCommands::Schema { db_type } => {
                commands::db::run_schema(db_type)?;
            }
        },

        Some(Commands::Agent(agent_cmd)) => {
            let ctx = ctx.as_ref().unwrap();
            match agent_cmd {
                cli::AgentCommands::Create {
                    non_interactive,
                    config,
                } => {
                    commands::agent::create::run(non_interactive, config)?;
                }
                cli::AgentCommands::Start {
                    agent_id,
                    config,
                    factory_type,
                    daemon,
                } => {
                    commands::agent::start::run(
                        ctx,
                        &agent_id,
                        config.as_deref(),
                        factory_type.as_deref(),
                        daemon,
                    )
                    .await?;
                }
                cli::AgentCommands::Stop {
                    agent_id,
                    force_persisted_stop,
                } => {
                    commands::agent::stop::run(ctx, &agent_id, force_persisted_stop).await?;
                }
                cli::AgentCommands::Restart { agent_id, config } => {
                    commands::agent::restart::run(ctx, &agent_id, config.as_deref()).await?;
                }
                cli::AgentCommands::Status { agent_id } => {
                    commands::agent::status::run(ctx, agent_id.as_deref()).await?;
                }
                cli::AgentCommands::List { running, all } => {
                    commands::agent::list::run(ctx, running, all).await?;
                }
                cli::AgentCommands::Logs {
                    agent_id,
                    tail,
                    level,
                    grep,
                    limit,
                    json,
                } => {
                    commands::agent::logs::run(
                        ctx,
                        &agent_id,
                        tail,
                        level.clone(),
                        grep.clone(),
                        limit,
                        json,
                    )
                    .await?;
                }
            }
        }
        Some(Commands::Config { action }) => match action {
            cli::ConfigCommands::Value(value_cmd) => match value_cmd {
                cli::ConfigValueCommands::Get { key } => {
                    commands::config_cmd::run_get(&key)?;
                }
                cli::ConfigValueCommands::Set { key, value } => {
                    commands::config_cmd::run_set(&key, &value)?;
                }
                cli::ConfigValueCommands::Unset { key } => {
                    commands::config_cmd::run_unset(&key)?;
                }
            },
            cli::ConfigCommands::List => {
                commands::config_cmd::run_list()?;
            }
            cli::ConfigCommands::Validate => {
                commands::config_cmd::run_validate(None)?;
            }
            cli::ConfigCommands::Path => {
                commands::config_cmd::run_path()?;
            }
        },

        Some(Commands::Plugin { action }) => {
            let ctx = ctx.as_ref().unwrap();
            match action {
                cli::PluginCommands::New { name } => {
                    commands::plugin::new::run(name.as_deref()).await?;
                }
                cli::PluginCommands::List {
                    installed,
                    available,
                } => {
                    commands::plugin::list::run(ctx, installed, available).await?;
                }
                cli::PluginCommands::Info { name } => {
                    commands::plugin::info::run(ctx, &name).await?;
                }
                cli::PluginCommands::Install {
                    name,
                    checksum,
                    verify_signature,
                } => {
                    commands::plugin::install::run(
                        ctx,
                        &name,
                        checksum.as_deref(),
                        verify_signature,
                    )
                    .await?;
                }
                cli::PluginCommands::Uninstall { name, force } => {
                    commands::plugin::uninstall::run(ctx, &name, force).await?;
                }
                cli::PluginCommands::Repository { action } => match action {
                    cli::PluginRepositoryCommands::List => {
                        commands::plugin::repository::list(ctx).await?;
                    }
                    cli::PluginRepositoryCommands::Add {
                        id,
                        url,
                        description,
                    } => {
                        commands::plugin::repository::add(ctx, &id, &url, description.as_deref())
                            .await?;
                    }
                },
            }
        }

        Some(Commands::Session { action }) => {
            let ctx = ctx.as_ref().unwrap();
            match action {
                cli::SessionCommands::List { agent, limit } => {
                    commands::session::list::run(ctx, agent.as_deref(), limit).await?;
                }
                cli::SessionCommands::Show { session_id, format } => {
                    let show_format = format.map(|f| f.to_string());
                    commands::session::show::run(ctx, &session_id, show_format.as_deref()).await?;
                }
                cli::SessionCommands::Delete { session_id, force } => {
                    commands::session::delete::run(ctx, &session_id, force).await?;
                }
                cli::SessionCommands::Export {
                    session_id,
                    output_path,
                    format,
                } => {
                    commands::session::export::run(
                        ctx,
                        &session_id,
                        output_path,
                        &format.to_string(),
                    )
                    .await?;
                }
            }
        }

        Some(Commands::Tool { action }) => {
            let ctx = ctx.as_ref().unwrap();
            match action {
                cli::ToolCommands::List { available, enabled } => {
                    commands::tool::list::run(ctx, available, enabled).await?;
                }
                cli::ToolCommands::Info { name } => {
                    commands::tool::info::run(ctx, &name).await?;
                }
            }
        }

        Some(Commands::Rag { action }) => match action {
            cli::RagCommands::Index {
                input,
                backend,
                index_file,
                dimensions,
                chunk_size,
                chunk_overlap,
                sentence_chunks,
                qdrant_url,
                qdrant_api_key,
                qdrant_collection,
            } => {
                commands::rag::run_index(
                    input,
                    &backend,
                    &index_file,
                    dimensions,
                    chunk_size,
                    chunk_overlap,
                    sentence_chunks,
                    qdrant_url.as_deref(),
                    qdrant_api_key.as_deref(),
                    &qdrant_collection,
                )
                .await?;
            }
            cli::RagCommands::Query {
                query,
                backend,
                index_file,
                dimensions,
                top_k,
                threshold,
                qdrant_url,
                qdrant_api_key,
                qdrant_collection,
            } => {
                commands::rag::run_query(
                    &query,
                    &backend,
                    &index_file,
                    dimensions,
                    top_k,
                    threshold,
                    qdrant_url.as_deref(),
                    qdrant_api_key.as_deref(),
                    &qdrant_collection,
                )
                .await?;
            }
        },

        Some(Commands::Swarm { action }) => match action {
            cli::SwarmCommands::Run {
                file,
                dry_run,
                metrics,
                pattern,
                timeout,
            } => {
                commands::swarm::run(&file, dry_run, metrics, pattern, timeout).await?;
            }
        },

        None => {
            // Should have been handled by TUI check above
            // If we get here, show help
            println!(
                "{}",
                "No command specified. Use --help for usage information.".yellow()
            );
            println!("Run with --tui flag or without arguments to launch the TUI.");
        }
    }

    Ok(())
}

fn normalize_legacy_output_flags(args: &mut [String]) {
    const TOP_LEVEL_COMMANDS: &[&str] = &[
        "new", "init", "build", "run", "dataflow", "generate", "info", "db", "agent", "config",
        "plugin", "session", "tool", "doctor", "rag",
    ];

    let top_command_index = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, arg)| TOP_LEVEL_COMMANDS.contains(&arg.as_str()))
        .map(|(idx, _)| idx);

    let top_command = top_command_index.and_then(|idx| args.get(idx).map(String::as_str));

    // Determine the subcommand: the token immediately after the top-level command that is not
    // a flag. Used to guard against normalising -o for subcommands with their own local -o flag.
    let sub_command = top_command_index.and_then(|cmd_idx| {
        args.get(cmd_idx + 1)
            .filter(|s| !s.starts_with('-'))
            .map(String::as_str)
    });

    let allows_global_after_command = match top_command {
        Some("info") | Some("agent") | Some("plugin") | Some("tool") | Some("config")
        | Some("build") | Some("run") | Some("init") => true,
        // `session show` and `session export` both define their own local -o flag, so skip
        // normalisation for those subcommands.  All other `session` subcommands (e.g. `list`)
        // use the global output-format flag and should be normalised.
        Some("session") => !matches!(sub_command, Some("show") | Some("export")),
        _ => false,
    };

    let mut i = 1;
    while i < args.len() {
        let before_command = match top_command_index {
            Some(cmd_idx) => i < cmd_idx,
            None => true,
        };
        let should_normalize = before_command || allows_global_after_command;

        // Handle --output=<format> and -o=<format> (single-token equals form).
        let equals_format = ["--output=", "-o="].iter().find_map(|prefix| {
            args[i]
                .strip_prefix(prefix)
                .filter(|v| is_output_format_value(v))
                .map(str::to_owned)
        });
        if let Some(fmt) = equals_format {
            if should_normalize {
                args[i] = format!("--output-format={fmt}");
            }
        } else if i + 1 < args.len() {
            // Handle --output <format> and -o <format> (space-separated form).
            let is_legacy_output_flag = args[i] == "-o" || args[i] == "--output";
            let looks_like_output_format = is_output_format_value(&args[i + 1]);

            if is_legacy_output_flag && looks_like_output_format && should_normalize {
                args[i] = "--output-format".to_string();
            }
        }

        i += 1;
    }
}

/// Returns `true` if `s` is a recognised global output-format value.
#[inline]
fn is_output_format_value(s: &str) -> bool {
    matches!(s, "text" | "json" | "table")
}

#[cfg(test)]
mod tests {
    use super::normalize_legacy_output_flags;

    #[test]
    fn test_normalize_legacy_output_before_command() {
        let mut args = vec![
            "mofa".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "info".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[1], "--output-format");
    }

    #[test]
    fn test_normalize_legacy_output_before_command_with_option_value_prefix() {
        let mut args = vec![
            "mofa".to_string(),
            "--config".to_string(),
            "/tmp/mofa.toml".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "info".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[3], "--output-format");
    }

    #[test]
    fn test_normalize_legacy_output_after_agent_command() {
        let mut args = vec![
            "mofa".to_string(),
            "agent".to_string(),
            "list".to_string(),
            "-o".to_string(),
            "table".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[3], "--output-format");
    }

    #[test]
    fn test_do_not_normalize_session_export_output_path() {
        let mut args = vec![
            "mofa".to_string(),
            "session".to_string(),
            "export".to_string(),
            "s1".to_string(),
            "-o".to_string(),
            "/tmp/s1.json".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[4], "-o");
    }

    #[test]
    fn test_do_not_normalize_session_show_local_output_alias() {
        let mut args = vec![
            "mofa".to_string(),
            "session".to_string(),
            "show".to_string(),
            "s1".to_string(),
            "-o".to_string(),
            "json".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[4], "-o");
    }

    // --- Fix 1: equals-sign form ---

    #[test]
    fn test_normalize_equals_form_before_command() {
        let mut args = vec![
            "mofa".to_string(),
            "--output=json".to_string(),
            "info".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[1], "--output-format=json");
    }

    #[test]
    fn test_normalize_short_equals_form_before_command() {
        let mut args = vec![
            "mofa".to_string(),
            "-o=table".to_string(),
            "info".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[1], "--output-format=table");
    }

    #[test]
    fn test_normalize_equals_form_after_agent_command() {
        let mut args = vec![
            "mofa".to_string(),
            "agent".to_string(),
            "list".to_string(),
            "--output=json".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[3], "--output-format=json");
    }

    #[test]
    fn test_do_not_normalize_equals_form_unknown_value() {
        // An unrecognised format value must not be touched.
        let mut args = vec![
            "mofa".to_string(),
            "--output=csv".to_string(),
            "info".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[1], "--output=csv");
    }

    // --- Fix 2: session list normalisation ---

    #[test]
    fn test_normalize_session_list_short_output_flag() {
        let mut args = vec![
            "mofa".to_string(),
            "session".to_string(),
            "list".to_string(),
            "-o".to_string(),
            "json".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[3], "--output-format");
    }

    #[test]
    fn test_normalize_session_list_long_output_flag() {
        let mut args = vec![
            "mofa".to_string(),
            "session".to_string(),
            "list".to_string(),
            "--output".to_string(),
            "table".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[3], "--output-format");
    }

    #[test]
    fn test_normalize_session_list_equals_form() {
        let mut args = vec![
            "mofa".to_string(),
            "session".to_string(),
            "list".to_string(),
            "--output=json".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[3], "--output-format=json");
    }

    #[test]
    fn test_do_not_normalize_session_show_equals_form() {
        let mut args = vec![
            "mofa".to_string(),
            "session".to_string(),
            "show".to_string(),
            "s1".to_string(),
            "--output=json".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        // session show has a local -o alias; the equals form must not be rewritten.
        assert_eq!(args[4], "--output=json");
    }

    #[test]
    fn test_do_not_normalize_session_export_equals_form() {
        let mut args = vec![
            "mofa".to_string(),
            "session".to_string(),
            "export".to_string(),
            "s1".to_string(),
            "-o=json".to_string(),
        ];
        normalize_legacy_output_flags(&mut args);
        assert_eq!(args[4], "-o=json");
    }
}
