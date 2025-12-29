//! BSL Analyzer - Language Server for 1C:Enterprise BSL.
//!
//! This is the main entry point for the LSP server.

use std::error::Error;

use clap::{Parser, Subcommand};

mod config;
mod handlers;
mod server;

#[derive(Parser)]
#[command(name = "bsl-analyzer")]
#[command(version)]
#[command(about = "BSL Language Server and Analyzer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run static analysis on a project
    Analyze {
        /// Path to the project root
        #[arg(short, long)]
        project: std::path::PathBuf,

        /// Output file for results
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,

        /// Output format (json, sarif, generic)
        #[arg(short, long, default_value = "json")]
        format: String,
    },

    /// Check configuration file
    CheckConfig {
        /// Path to configuration file
        #[arg(short, long)]
        config: std::path::PathBuf,
    },

    /// Start LSP server (default)
    Lsp,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Analyze { project, output, format }) => analyze(project, output, format),
        Some(Commands::CheckConfig { config }) => check_config(config),
        Some(Commands::Lsp) | None => run_lsp_server(),
    }
}

fn run_lsp_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Starting BSL Analyzer LSP server");

    // TODO: Implement LSP server using lsp-server crate
    // For now, just print a message

    eprintln!("BSL Analyzer LSP server is not yet fully implemented.");
    eprintln!("Please check the project roadmap for implementation status.");

    Ok(())
}

fn analyze(
    project: std::path::PathBuf,
    output: Option<std::path::PathBuf>,
    format: String,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Analyzing project: {:?}", project);
    tracing::info!("Output format: {}", format);

    // TODO: Implement analysis

    if let Some(output) = output {
        tracing::info!("Writing results to: {:?}", output);
    }

    Ok(())
}

fn check_config(config: std::path::PathBuf) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Checking configuration: {:?}", config);

    let content = std::fs::read_to_string(&config)?;
    let _config: project_model::ProjectConfig = serde_json::from_str(&content)?;

    println!("Configuration is valid!");

    Ok(())
}
