//! BSL Analyzer - Language Server for 1C:Enterprise BSL.
//!
//! This is the main entry point for the LSP server.

use std::{env, error::Error, fs, io, path::PathBuf, process::Command, sync::Arc};

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
        /// Path to the project root (Java-compatible alias: --src)
        #[arg(short, long, alias = "src")]
        project: std::path::PathBuf,

        /// Output file for results
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,

        /// Output format: json, sonarqube, sarif, generic (Java-compatible alias: --reporter)
        #[arg(short, long, alias = "reporter", default_value = "json")]
        format: String,

        /// Enable incremental analysis (only analyze affected modules)
        #[arg(long)]
        incremental: bool,

        /// Comma-separated list of changed files (for incremental mode)
        #[arg(long, value_delimiter = ',', requires = "incremental")]
        changed_files: Option<Vec<PathBuf>>,

        /// Git ref to compare against (e.g., HEAD~1, origin/main) for incremental mode
        #[arg(long, requires = "incremental", conflicts_with = "changed_files")]
        git_diff: Option<String>,
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
        Some(Commands::Analyze {
            project,
            output,
            format,
            incremental,
            changed_files,
            git_diff,
        }) => analyze(project, output, format, incremental, changed_files, git_diff),
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
    project: PathBuf,
    output: Option<PathBuf>,
    format: String,
    incremental: bool,
    changed_files: Option<Vec<PathBuf>>,
    git_diff: Option<String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Analyzing project: {:?}", project);
    tracing::info!("Output format: {}", format);
    tracing::info!("Incremental mode: {}", incremental);

    // TODO: Full analysis implementation will be in later iterations
    // For now, we demonstrate the incremental workflow

    if incremental {
        tracing::info!("Running incremental analysis");

        // Resolve changed files
        let changed_paths = if let Some(files) = changed_files {
            tracing::info!("Using explicitly provided changed files: {} files", files.len());
            files
        } else if let Some(git_ref) = git_diff {
            tracing::info!("Getting changed files from git diff {}", git_ref);
            get_changed_files_from_git(&project, &git_ref)?
        } else {
            return Err("Incremental mode requires either --changed-files or --git-diff".into());
        };

        tracing::info!("Changed files: {} files", changed_paths.len());
        for path in &changed_paths {
            tracing::debug!("  - {:?}", path);
        }

        // TODO: Build module graph, compute affected modules, run analysis
        // This will be implemented when we integrate with ide-diagnostics
        println!(
            "Incremental analysis would process {} changed files + affected modules",
            changed_paths.len()
        );
    } else {
        tracing::info!("Running full analysis");
        println!("Full analysis mode");
    }

    if let Some(output) = output {
        tracing::info!("Results would be written to: {:?}", output);
        println!("Output: {:?} (format: {})", output, format);
    }

    Ok(())
}

/// Retrieves changed files from git diff.
fn get_changed_files_from_git(
    project_root: &PathBuf,
    git_ref: &str,
) -> Result<Vec<PathBuf>, Box<dyn Error + Send + Sync>> {
    let output = Command::new("git")
        .current_dir(project_root)
        .args(["diff", "--name-only", git_ref])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr).into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let files: Vec<PathBuf> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter(|line| line.ends_with(".bsl")) // Only BSL files
        .map(|line| project_root.join(line))
        .collect();

    Ok(files)
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
