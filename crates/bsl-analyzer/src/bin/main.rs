//! BSL Analyzer - Language Server for 1C:Enterprise BSL.
//!
//! This is the main entry point for the LSP server.

use std::{env, error::Error, fs, io, path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};

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
    let log_file = env::var("BSL_LOG_FILE").ok().map(PathBuf::from);
    setup_logging(log_file)?;

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Analyze { project, output, format }) => analyze(project, output, format),
        Some(Commands::CheckConfig { config }) => check_config(config),
        Some(Commands::Lsp) | None => run_lsp_server(),
    }
}

fn run_lsp_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Starting BSL Analyzer LSP server");

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

fn setup_logging(log_file: Option<PathBuf>) -> anyhow::Result<()> {
    use tracing_subscriber::fmt::writer::BoxMakeWriter;

    let writer: BoxMakeWriter = match log_file {
        Some(file) => BoxMakeWriter::new(Arc::new(fs::File::create(&file)?)),
        None => BoxMakeWriter::new(io::stderr),
    };

    bsl_analyzer::tracing::Config {
        writer,
        filter: env::var("BSL_LOG").ok().unwrap_or_else(|| "warn".to_owned()),
        profile_filter: env::var("BSL_PROFILE").ok(),
    }
    .init()?;

    Ok(())
}
