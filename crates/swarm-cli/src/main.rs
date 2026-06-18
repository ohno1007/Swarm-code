//! Swarm-code CLI entry point.

mod keystore;
mod repl;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use swarm_analyzer::Analyzer;

#[derive(Parser)]
#[command(name = "swarm", version, about = "Swarm-code: a multi-agent AI coding CLI")]
struct Cli {
    /// Workspace root the agents operate within (default: current dir).
    #[arg(short, long, global = true)]
    workspace: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start an interactive multi-session chat (default).
    Chat,
    /// Run a single task with the lead agent and print the result.
    Run {
        /// The task description.
        task: String,
    },
    /// Analyze a source file and print its symbol/scope outline.
    Analyze {
        /// File to analyze.
        path: PathBuf,
        /// Emit the full symbol tree as JSON instead of an outline.
        #[arg(long)]
        json: bool,
    },
    /// Manage credentials (DeepSeek API key).
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set and save the DeepSeek API key.
    SetKey,
    /// Show the current (masked) configuration.
    Show,
    /// Print the config file path.
    Path,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "swarm_core=info,swarm_cli=info".into()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Config and analyze don't need a key or a workspace handshake.
    match cli.command.unwrap_or(Command::Chat) {
        Command::Config { action } => return config(action),
        Command::Analyze { path, json } => return analyze(&path, json),
        Command::Run { task } => {
            keystore::ensure_api_key()?;
            run_once(resolve_workspace(cli.workspace)?, task).await
        }
        Command::Chat => {
            keystore::ensure_api_key()?;
            repl::run(resolve_workspace(cli.workspace)?).await
        }
    }
}

fn resolve_workspace(arg: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    Ok(arg.unwrap_or(std::env::current_dir()?).canonicalize()?)
}

fn config(action: ConfigAction) -> anyhow::Result<()> {
    match action {
        ConfigAction::SetKey => keystore::set_key_interactive(),
        ConfigAction::Show => {
            keystore::show();
            Ok(())
        }
        ConfigAction::Path => {
            println!("{}", keystore::config_path().display());
            Ok(())
        }
    }
}

fn analyze(path: &std::path::Path, json: bool) -> anyhow::Result<()> {
    let analyzer = Analyzer::new();
    let root = analyzer.analyze_path(path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&root)?);
    } else {
        print!("{}", swarm_analyzer::render_outline(&root));
    }
    Ok(())
}

async fn run_once(workspace: PathBuf, task: String) -> anyhow::Result<()> {
    let manager = swarm_core::build_manager(workspace)?;
    let id = manager.create("run").await;
    let answer = manager.send(id, task).await?;
    println!("{answer}");
    Ok(())
}
